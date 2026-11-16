//! O executor: varre o cofre, deixa o motor decidir e aplica as ações com segurança.
//!
//! Ordem de um ciclo:
//! 1. **Retomar** o diário (intenções que ficaram no meio por queda de rede ou de processo).
//! 2. **Puxar** as mudanças do servidor para a caixa de entrada (`remoto`) e avançar o cursor.
//! 3. **Varrer** o cofre e **decifrar** o que o servidor tem de novo (a caixa de entrada
//!    menos o que a base já incorpora).
//! 4. **Decidir** com `motor::reconciliar`.
//! 5. Gravar o **plano inteiro no diário** e só então aplicar, uma intenção por vez; cada
//!    intenção termina junto com a atualização da base, numa transação.
//!
//! Regras de segurança dos efeitos:
//! - escrita atômica (temporário + rename), conferindo antes que o arquivo não mudou desde a
//!   varredura; se mudou, o que estava lá é guardado como conflito antes de sobrescrever;
//! - apagar é mover para `.trash/`;
//! - todo caminho que sai de `decifrar` passa de novo por `caminho_valido` e o id tem de bater;
//! - nenhum componente do caminho pode ser symlink.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use chrono::NaiveDateTime;
use nota_cripto::{ChaveCofre, IdArquivo};

use crate::caminhos::{caminho_valido, nome_conflito};
use crate::cofre::{self, hash_de};
use crate::erro::ErroSync;
use crate::estado::{Estado, FaseEnvio, Intencao};
use crate::motor::{self, Acao, EntradaBase, EstadoBase, EstadoLocal, Hash, MudancaRemota};
use crate::protocolo::{Conflito409, ItemRemoto, hex64, sha256_hex};
use crate::remoto::Remoto;

/// Conteúdo até este tamanho fica em memória entre a decifragem e a escrita.
const CACHE_MAXIMO: usize = 1024 * 1024;
/// Limite da guarda contra exclusão em massa (cofre vazio ou desmontado).
const MASSA_MINIMA: usize = 20;

/// Onde ficam as coisas deste aparelho.
#[derive(Debug, Clone)]
pub struct Opcoes {
    /// Pasta do cofre (tem de existir).
    pub cofre: PathBuf,
    /// Arquivo do banco de estado (redb). **Fora do cofre**: copiar o cofre não pode copiar o estado.
    pub estado: PathBuf,
    /// Nome do aparelho (entra no nome das cópias de conflito).
    pub aparelho: String,
}

/// O que um ciclo fez.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relatorio {
    /// Intenções executadas (inclui as retomadas do diário).
    pub acoes: usize,
    pub enviados: usize,
    pub baixados: usize,
    pub conflitos: usize,
    pub apagados_local: usize,
    pub lapides: usize,
    /// O que foi recusado ou deixado para depois, e por quê (sem conteúdo de nota).
    pub recusados: Vec<String>,
}

/// Função chamada logo antes de cada efeito (número da execução, intenção). Só para testes.
pub type GanchoEfeito = Box<dyn FnMut(usize, &Intencao) + Send>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Momento {
    Antes,
    Depois,
}

pub struct Sincronizador<R: Remoto> {
    opcoes: Opcoes,
    chave: ChaveCofre,
    remoto: R,
    estado: Estado,
    relogio: Box<dyn FnMut() -> NaiveDateTime + Send>,
    agora: NaiveDateTime,
    permitir_massa: bool,
    cache: HashMap<String, Vec<u8>>,
    rel: Relatorio,
    execucoes: usize,
    queda: Option<(usize, Momento)>,
    gancho_efeito: Option<GanchoEfeito>,
}

fn io(e: std::io::Error) -> ErroSync {
    ErroSync::from(e)
}

impl<R: Remoto> Sincronizador<R> {
    pub fn novo(opcoes: Opcoes, chave: ChaveCofre, remoto: R) -> Result<Self, ErroSync> {
        if !opcoes.cofre.is_dir() {
            return Err(ErroSync::Io(format!(
                "o cofre não é uma pasta: {}",
                opcoes.cofre.display()
            )));
        }
        let estado = Estado::abrir(&opcoes.estado)?;
        let mut relogio: Box<dyn FnMut() -> NaiveDateTime + Send> =
            Box::new(|| chrono::Local::now().naive_local());
        let agora = relogio();
        Ok(Self {
            opcoes,
            chave,
            remoto,
            estado,
            relogio,
            agora,
            permitir_massa: false,
            cache: HashMap::new(),
            rel: Relatorio::default(),
            execucoes: 0,
            queda: None,
            gancho_efeito: None,
        })
    }

    /// Troca o relógio (testes: tempo controlado).
    pub fn com_relogio(mut self, relogio: impl FnMut() -> NaiveDateTime + Send + 'static) -> Self {
        self.relogio = Box::new(relogio);
        self
    }

    /// Libera a guarda contra exclusão em massa (use só quando o usuário apagou de propósito).
    pub fn permitir_exclusao_em_massa(&mut self, permitir: bool) {
        self.permitir_massa = permitir;
    }

    pub fn remoto(&mut self) -> &mut R {
        &mut self.remoto
    }

    pub fn estado(&self) -> &Estado {
        &self.estado
    }

    /// Simula uma queda do processo na execução de número `n` (0, 1, ...) do próximo trecho:
    /// antes do efeito ou depois dele, mas sempre antes de a intenção terminar. Só para testes.
    #[doc(hidden)]
    pub fn injetar_queda(&mut self, n: usize, depois_do_efeito: bool) {
        self.execucoes = 0;
        self.queda = Some((
            n,
            if depois_do_efeito {
                Momento::Depois
            } else {
                Momento::Antes
            },
        ));
    }

    #[doc(hidden)]
    pub fn limpar_queda(&mut self) {
        self.queda = None;
    }

    /// Instala (ou tira) uma função chamada logo antes de cada efeito, depois da varredura:
    /// o teste mexe no cofre exatamente entre a decisão e a aplicação. Só para testes.
    #[doc(hidden)]
    pub fn definir_gancho_efeito(&mut self, gancho: Option<GanchoEfeito>) {
        self.gancho_efeito = gancho;
    }

    fn carimbo(&self) -> String {
        self.agora.format("%Y-%m-%d %H%M").to_string()
    }

    fn carimbo_lixeira(&self) -> String {
        self.agora.format("%Y-%m-%d %H%M%S").to_string()
    }

    fn recusar(&mut self, motivo: String) {
        let _ = self.estado.registrar(&motivo);
        self.rel.recusados.push(motivo);
    }

    /// Um ciclo completo. Em caso de erro de rede ou de estado, o que já foi feito está
    /// registrado e o resto fica no diário para o próximo ciclo.
    pub fn ciclo(&mut self) -> Result<Relatorio, ErroSync> {
        self.agora = (self.relogio)();
        self.rel = Relatorio::default();
        self.cache.clear();

        self.executar_diario()?;
        self.puxar()?;

        let varredura = cofre::varrer(&self.opcoes.cofre).map_err(io)?;
        for r in &varredura.recusados {
            self.recusar(r.clone());
        }
        let mut locais = Vec::with_capacity(varredura.arquivos.len());
        for a in &varredura.arquivos {
            match self.chave.id_de(&a.caminho) {
                Ok(id) => locais.push(EstadoLocal {
                    caminho: a.caminho.clone(),
                    id: id.0,
                    hash: a.hash,
                }),
                Err(e) => self.recusar(format!("caminho sem id ({e}): {}", a.caminho)),
            }
        }

        let base = self.estado.base()?;
        let caixa = self.estado.remotos()?;
        let mut mudancas: Vec<MudancaRemota> = Vec::new();
        let mut blobs: BTreeMap<String, String> = BTreeMap::new();
        for item in caixa.values() {
            let versao_base = base.get(&item.id).map_or(0, |e| e.versao);
            if item.versao <= versao_base {
                continue;
            }
            if item.apagado {
                mudancas.push(MudancaRemota {
                    id: item.id.clone(),
                    versao: item.versao,
                    apagado: true,
                    caminho: base.get(&item.id).and_then(|e| e.caminho.clone()),
                    hash: None,
                });
                continue;
            }
            let Some(hash_blob) = item.hash_blob.clone() else {
                self.recusar(format!(
                    "item {} sem blob",
                    &item.id[..8.min(item.id.len())]
                ));
                continue;
            };
            match self.conteudo_remoto(&item.id, &hash_blob)? {
                Ok((caminho, bytes)) => {
                    mudancas.push(MudancaRemota {
                        id: item.id.clone(),
                        versao: item.versao,
                        apagado: false,
                        caminho: Some(caminho),
                        hash: Some(hash_de(&bytes)),
                    });
                    blobs.insert(item.id.clone(), hash_blob);
                    if bytes.len() <= CACHE_MAXIMO {
                        self.cache.insert(item.id.clone(), bytes);
                    }
                }
                Err(motivo) => self.recusar(motivo),
            }
        }

        let estado_base = EstadoBase {
            aparelho: self.opcoes.aparelho.clone(),
            carimbo: self.carimbo(),
            entradas: base.clone(),
        };
        let acoes = motor::reconciliar(&locais, &estado_base, &mudancas);

        // Guarda: muita exclusão de uma vez costuma ser cofre vazio ou desmontado.
        let lapides = acoes
            .iter()
            .filter(|a| matches!(a, Acao::EnviarLapide { .. }))
            .count();
        let total = base.values().filter(|e| e.hash.is_some()).count();
        if !self.permitir_massa && lapides >= MASSA_MINIMA && lapides * 2 >= total {
            return Err(ErroSync::ExclusaoEmMassa { lapides, total });
        }

        let por_caminho: BTreeMap<&str, &EstadoLocal> =
            locais.iter().map(|l| (l.caminho.as_str(), l)).collect();
        let por_id: BTreeMap<&str, &EstadoLocal> =
            locais.iter().map(|l| (l.id.as_str(), l)).collect();
        let remotas: BTreeMap<&str, &MudancaRemota> =
            mudancas.iter().map(|m| (m.id.as_str(), m)).collect();
        let versao_conhecida = |id: &str| -> u64 {
            let b = base.get(id).map_or(0, |e| e.versao);
            let r = caixa.get(id).map_or(0, |i| i.versao);
            b.max(r)
        };

        let mut plano: Vec<Intencao> = Vec::new();
        let mut com_acao: BTreeSet<String> = BTreeSet::new();
        for acao in &acoes {
            match acao {
                Acao::Enviar { caminho } => {
                    let Some(l) = por_caminho.get(caminho.as_str()) else {
                        continue;
                    };
                    com_acao.insert(l.id.clone());
                    plano.push(Intencao::Enviar {
                        id: l.id.clone(),
                        caminho: caminho.clone(),
                        vbase: versao_conhecida(&l.id),
                        fase: FaseEnvio::Inicio,
                    });
                }
                Acao::EnviarLapide { id } => {
                    com_acao.insert(id.clone());
                    plano.push(Intencao::EnviarLapide {
                        id: id.clone(),
                        caminho: base.get(id).and_then(|e| e.caminho.clone()),
                        vbase: versao_conhecida(id),
                    });
                }
                Acao::Baixar { id } => {
                    let (Some(m), Some(blob)) = (remotas.get(id.as_str()), blobs.get(id)) else {
                        continue;
                    };
                    let (Some(caminho), Some(hash_remoto)) = (m.caminho.clone(), m.hash) else {
                        continue;
                    };
                    com_acao.insert(id.clone());
                    plano.push(Intencao::Baixar {
                        id: id.clone(),
                        caminho,
                        rv: m.versao,
                        hash_blob: blob.clone(),
                        hash_remoto,
                        esperado: por_id.get(id.as_str()).map(|l| l.hash),
                    });
                }
                Acao::BaixarComo { id, nome } => {
                    let (Some(m), Some(blob)) = (remotas.get(id.as_str()), blobs.get(id)) else {
                        continue;
                    };
                    let (Some(caminho_remoto), Some(hash_remoto)) = (m.caminho.clone(), m.hash)
                    else {
                        continue;
                    };
                    com_acao.insert(id.clone());
                    plano.push(Intencao::BaixarComo {
                        id: id.clone(),
                        caminho_remoto,
                        nome: nome.clone(),
                        rv: m.versao,
                        hash_blob: blob.clone(),
                        hash_remoto,
                    });
                }
                Acao::ApagarLocal { caminho } => {
                    let Some(l) = por_caminho.get(caminho.as_str()) else {
                        continue;
                    };
                    com_acao.insert(l.id.clone());
                    plano.push(Intencao::ApagarLocal {
                        id: l.id.clone(),
                        caminho: caminho.clone(),
                        rv: remotas
                            .get(l.id.as_str())
                            .filter(|m| m.apagado)
                            .map(|m| m.versao),
                        esperado: l.hash,
                    });
                }
                Acao::GravarConflito {
                    caminho,
                    nome_conflito,
                } => {
                    let Some(l) = por_caminho.get(caminho.as_str()) else {
                        continue;
                    };
                    plano.push(Intencao::GravarConflito {
                        caminho: caminho.clone(),
                        nome: nome_conflito.clone(),
                        esperado: l.hash,
                    });
                }
            }
        }

        // Lápide remota que não pede ação (apagado nos dois lados, ou arquivo que nunca
        // existiu aqui): só anota a versão na base.
        for m in mudancas.iter().filter(|m| m.apagado) {
            if !com_acao.contains(&m.id) {
                let entrada = EntradaBase {
                    id: m.id.clone(),
                    caminho: m.caminho.clone(),
                    versao: m.versao,
                    hash: None,
                };
                self.estado.gravar_base(&entrada, None)?;
            }
        }

        self.estado.gravar_plano(&plano)?;
        self.executar_diario()?;
        Ok(std::mem::take(&mut self.rel))
    }

    /// Recebe as mudanças do servidor para a caixa de entrada.
    fn puxar(&mut self) -> Result<(), ErroSync> {
        let mut cursor = self.estado.seq()?;
        loop {
            let m = self.remoto.mudancas(cursor)?;
            let itens: Vec<ItemRemoto> = m
                .itens
                .into_iter()
                .filter(|i| {
                    hex64(&i.id)
                        && (i.apagado || i.hash_blob.as_deref().is_some_and(hex64))
                        && i.versao > 0
                })
                .collect();
            self.estado.guardar_remoto(&itens, m.seq)?;
            if !m.mais || m.seq <= cursor {
                return Ok(());
            }
            cursor = m.seq;
        }
    }

    /// Baixa e abre um blob. `Err(String)` interno = recusa permanente deste blob (adulterado,
    /// caminho perigoso, id que não bate); rede e disco viram `Err(ErroSync)` externo.
    fn conteudo_remoto(
        &mut self,
        id: &str,
        hash_blob: &str,
    ) -> Result<Result<(String, Vec<u8>), String>, ErroSync> {
        let curto = &id[..8.min(id.len())];
        let blob = match self.remoto.baixar_blob(hash_blob) {
            Ok(b) => b,
            // O índice cita um blob que o servidor não tem (backup antigo, disco trocado):
            // recusa só este item, o resto do cofre continua sincronizando.
            Err(crate::remoto::ErroRemoto::NaoEncontrado) => {
                return Ok(Err(format!("blob do item {curto} ausente no servidor")));
            }
            Err(e) => return Err(e.into()),
        };
        if sha256_hex(&blob) != hash_blob {
            return Err(ErroSync::Remoto(crate::remoto::ErroRemoto::Protocolo(
                "o blob recebido não bate com o hash".into(),
            )));
        }
        let (caminho, bytes) = match self.chave.decifrar(&IdArquivo(id.to_string()), &blob) {
            Ok(x) => x,
            Err(e) => return Ok(Err(format!("blob do item {curto} não abre: {e}"))),
        };
        if !caminho_valido(&caminho) {
            return Ok(Err(format!(
                "item {curto} recusado: caminho perigoso ou inválido"
            )));
        }
        match self.chave.id_de(&caminho) {
            Ok(esperado) if esperado.0 == id => {}
            _ => {
                return Ok(Err(format!(
                    "item {curto} recusado: o caminho não bate com o id"
                )));
            }
        }
        Ok(Ok((caminho, bytes)))
    }

    /// Conteúdo para uma intenção de baixar: do cache ou do servidor, conferido de novo.
    fn obter_conteudo(
        &mut self,
        id: &str,
        caminho: &str,
        hash_blob: &str,
        hash_remoto: &Hash,
    ) -> Result<Result<Vec<u8>, String>, ErroSync> {
        if let Some(b) = self.cache.get(id)
            && hash_de(b) == *hash_remoto
        {
            return Ok(Ok(b.clone()));
        }
        match self.conteudo_remoto(id, hash_blob)? {
            Ok((c, bytes)) if c == caminho && hash_de(&bytes) == *hash_remoto => Ok(Ok(bytes)),
            Ok(_) => Ok(Err(format!(
                "item {} mudou de caminho ou conteúdo no meio do ciclo",
                &id[..8.min(id.len())]
            ))),
            Err(m) => Ok(Err(m)),
        }
    }

    /// Executa o diário pendente, em ordem.
    fn executar_diario(&mut self) -> Result<(), ErroSync> {
        for (chave, intencao) in self.estado.diario()? {
            let n = self.execucoes;
            self.execucoes += 1;
            if self.queda == Some((n, Momento::Antes)) {
                return Err(ErroSync::Interrompido);
            }
            self.rel.acoes += 1;
            if let Some(g) = self.gancho_efeito.as_mut() {
                g(n, &intencao);
            }
            match self.executar(chave, &intencao) {
                Ok(()) => {}
                Err(ErroSync::Io(m)) => {
                    // Falha de disco: não trava os outros nem repete para sempre.
                    self.recusar(format!("falha de disco: {m}"));
                    self.estado.concluir(chave, None, None)?;
                }
                Err(e) => return Err(e),
            }
            if self.queda == Some((n, Momento::Depois)) {
                return Err(ErroSync::Interrompido);
            }
        }
        Ok(())
    }

    fn real(&self, caminho: &str) -> String {
        cofre::achar_real(&self.opcoes.cofre, caminho)
    }

    fn ler_local(&self, caminho: &str) -> Result<Option<Vec<u8>>, ErroSync> {
        cofre::ler(&self.opcoes.cofre, &self.real(caminho)).map_err(io)
    }

    fn executar(&mut self, chave: u64, intencao: &Intencao) -> Result<(), ErroSync> {
        match intencao.clone() {
            Intencao::Enviar {
                id,
                caminho,
                vbase,
                fase,
            } => self.exec_enviar(chave, id, caminho, vbase, fase),
            Intencao::EnviarLapide { id, caminho, vbase } => {
                self.exec_lapide(chave, id, caminho, vbase)
            }
            Intencao::Baixar {
                id,
                caminho,
                rv,
                hash_blob,
                hash_remoto,
                esperado,
            } => self.exec_baixar(chave, id, caminho, rv, hash_blob, hash_remoto, esperado),
            Intencao::ApagarLocal {
                id,
                caminho,
                rv,
                esperado,
            } => self.exec_apagar(chave, id, caminho, rv, esperado),
            Intencao::GravarConflito {
                caminho,
                nome,
                esperado: _,
            } => self.exec_conflito(chave, caminho, nome),
            Intencao::BaixarComo {
                id,
                caminho_remoto,
                nome,
                rv,
                hash_blob,
                hash_remoto,
            } => self.exec_baixar_como(chave, id, caminho_remoto, nome, rv, hash_blob, hash_remoto),
        }
    }

    /// Termina a intenção. Na queda simulada "depois", o efeito já aconteceu mas a conclusão
    /// não é gravada: o diário fica como numa queda de verdade entre o efeito e o registro.
    fn concluir(
        &mut self,
        chave: u64,
        base: Option<&EntradaBase>,
        remoto: Option<&ItemRemoto>,
    ) -> Result<(), ErroSync> {
        if self.queda == Some((self.execucoes - 1, Momento::Depois)) {
            return Ok(()); // `executar_diario` devolve `Interrompido` em seguida
        }
        self.estado.concluir(chave, base, remoto)
    }

    fn exec_enviar(
        &mut self,
        chave: u64,
        id: String,
        caminho: String,
        vbase: u64,
        fase: FaseEnvio,
    ) -> Result<(), ErroSync> {
        let (hash_blob, hash_texto) = match fase {
            FaseEnvio::Enviado {
                hash_blob,
                hash_texto,
            } => (hash_blob, hash_texto),
            FaseEnvio::Inicio => {
                let Some(bytes) = self.ler_local(&caminho)? else {
                    self.recusar(format!("{caminho}: sumiu antes de enviar"));
                    return self.concluir(chave, None, None);
                };
                let id_ok = matches!(self.chave.id_de(&caminho), Ok(i) if i.0 == id);
                if !caminho_valido(&caminho) || !id_ok {
                    self.recusar(format!("{caminho}: caminho recusado no envio"));
                    return self.concluir(chave, None, None);
                }
                let hash_texto = hash_de(&bytes);
                let blob = match self.chave.cifrar(&caminho, &bytes) {
                    Ok(b) => b,
                    Err(e) => {
                        self.recusar(format!("{caminho}: não cifrou ({e})"));
                        return self.concluir(chave, None, None);
                    }
                };
                let hash_blob = sha256_hex(&blob);
                let devolvido = self.remoto.enviar_blob(&blob)?;
                if devolvido != hash_blob {
                    return Err(ErroSync::Remoto(crate::remoto::ErroRemoto::Protocolo(
                        "hash do blob diferente do enviado".into(),
                    )));
                }
                let nova = Intencao::Enviar {
                    id: id.clone(),
                    caminho: caminho.clone(),
                    vbase,
                    fase: FaseEnvio::Enviado {
                        hash_blob: hash_blob.clone(),
                        hash_texto,
                    },
                };
                self.estado.atualizar_intencao(chave, &nova)?;
                (hash_blob, hash_texto)
            }
        };
        let resposta = match self.remoto.publicar(&id, vbase, &hash_blob, false) {
            Ok(r) => r,
            Err(e @ crate::remoto::ErroRemoto::Protocolo(_)) => {
                // O servidor não aceitou (por exemplo, perdeu o blob): volta ao início para
                // reenviar o blob em vez de repetir para sempre a mesma publicação.
                let volta = Intencao::Enviar {
                    id: id.clone(),
                    caminho: caminho.clone(),
                    vbase,
                    fase: FaseEnvio::Inicio,
                };
                self.estado.atualizar_intencao(chave, &volta)?;
                return Err(e.into());
            }
            Err(e) => return Err(e.into()),
        };
        let versao = match resposta {
            Ok(v) => v,
            // Nossa publicação anterior chegou e só a resposta se perdeu.
            Err(c) if !c.apagado && c.hash_blob.as_deref() == Some(hash_blob.as_str()) => {
                c.versao_atual
            }
            Err(c) => {
                // Alguém publicou antes: a caixa de entrada aprende e o próximo ciclo decide.
                self.rel.recusados.push(format!(
                    "{caminho}: o servidor já estava na versão {}; decido no próximo ciclo",
                    c.versao_atual
                ));
                let item = item_de(&id, &c);
                return self.concluir(chave, None, Some(&item));
            }
        };
        self.rel.enviados += 1;
        let entrada = EntradaBase {
            id: id.clone(),
            caminho: Some(caminho),
            versao,
            hash: Some(hash_texto),
        };
        let item = ItemRemoto {
            id,
            versao,
            hash_blob: Some(hash_blob),
            apagado: false,
            seq: 0,
        };
        self.concluir(chave, Some(&entrada), Some(&item))
    }

    fn exec_lapide(
        &mut self,
        chave: u64,
        id: String,
        caminho: Option<String>,
        vbase: u64,
    ) -> Result<(), ErroSync> {
        let resposta = self.remoto.publicar(&id, vbase, "", true)?;
        let (versao, item) = match resposta {
            Ok(v) => (
                v,
                ItemRemoto {
                    id: id.clone(),
                    versao: v,
                    hash_blob: None,
                    apagado: true,
                    seq: 0,
                },
            ),
            // Já estava apagado (por nós, antes da queda, ou por outro aparelho): serve.
            Err(c) if c.apagado => (c.versao_atual, item_de(&id, &c)),
            Err(c) => {
                self.rel.recusados.push(format!(
                    "lápide adiada: o servidor mudou (versão {})",
                    c.versao_atual
                ));
                let item = item_de(&id, &c);
                return self.concluir(chave, None, Some(&item));
            }
        };
        self.rel.lapides += 1;
        let entrada = EntradaBase {
            id,
            caminho,
            versao,
            hash: None,
        };
        self.concluir(chave, Some(&entrada), Some(&item))
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_baixar(
        &mut self,
        chave: u64,
        id: String,
        caminho: String,
        rv: u64,
        hash_blob: String,
        hash_remoto: Hash,
        esperado: Option<Hash>,
    ) -> Result<(), ErroSync> {
        let bytes = match self.obter_conteudo(&id, &caminho, &hash_blob, &hash_remoto)? {
            Ok(b) => b,
            Err(m) => {
                self.recusar(m);
                return self.concluir(chave, None, None);
            }
        };
        let entrada = EntradaBase {
            id: id.clone(),
            caminho: Some(caminho.clone()),
            versao: rv,
            hash: Some(hash_remoto),
        };
        let atual = self.ler_local(&caminho)?;
        let hash_atual = atual.as_ref().map(|b| hash_de(b));
        if hash_atual == Some(hash_remoto) {
            // Já é igual (criado igual nos dois lados, ou o rename já tinha acontecido).
            return self.concluir(chave, Some(&entrada), None);
        }
        if hash_atual != esperado {
            // Mudou entre a varredura e agora: o que está lá é do usuário e vira conflito.
            if let Some(a) = &atual {
                let nome = nome_conflito(&caminho, &self.opcoes.aparelho, &self.carimbo());
                cofre::gravar_unico(&self.opcoes.cofre, &nome, a).map_err(io)?;
                self.rel.conflitos += 1;
            }
        }
        let alvo = self.real(&caminho);
        cofre::escrever_atomico(&self.opcoes.cofre, &alvo, &bytes).map_err(io)?;
        self.rel.baixados += 1;
        self.concluir(chave, Some(&entrada), None)
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_baixar_como(
        &mut self,
        chave: u64,
        id: String,
        caminho_remoto: String,
        nome: String,
        rv: u64,
        hash_blob: String,
        hash_remoto: Hash,
    ) -> Result<(), ErroSync> {
        let bytes = match self.obter_conteudo(&id, &caminho_remoto, &hash_blob, &hash_remoto)? {
            Ok(b) => b,
            Err(m) => {
                self.recusar(m);
                return self.concluir(chave, None, None);
            }
        };
        cofre::gravar_unico(&self.opcoes.cofre, &nome, &bytes).map_err(io)?;
        self.rel.conflitos += 1;
        // O id fica registrado sem conteúdo sob este caminho: o arquivo, com outro nome,
        // entra no próximo ciclo como arquivo novo e vai para o servidor com id próprio.
        let entrada = EntradaBase {
            id,
            caminho: Some(caminho_remoto),
            versao: rv,
            hash: None,
        };
        self.concluir(chave, Some(&entrada), None)
    }

    fn exec_apagar(
        &mut self,
        chave: u64,
        id: String,
        caminho: String,
        rv: Option<u64>,
        esperado: Hash,
    ) -> Result<(), ErroSync> {
        let entrada = rv.map(|versao| EntradaBase {
            id: id.clone(),
            caminho: Some(caminho.clone()),
            versao,
            hash: None,
        });
        let Some(atual) = self.ler_local(&caminho)? else {
            return self.concluir(chave, entrada.as_ref(), None); // já não está lá
        };
        if hash_de(&atual) != esperado {
            self.recusar(format!(
                "{caminho}: mudou desde a varredura, não vai para a lixeira; decido no próximo ciclo"
            ));
            return self.concluir(chave, None, None);
        }
        let real = self.real(&caminho);
        cofre::mover_para_lixeira(&self.opcoes.cofre, &real, &self.carimbo_lixeira())
            .map_err(io)?;
        self.rel.apagados_local += 1;
        self.concluir(chave, entrada.as_ref(), None)
    }

    fn exec_conflito(&mut self, chave: u64, caminho: String, nome: String) -> Result<(), ErroSync> {
        let Some(atual) = self.ler_local(&caminho)? else {
            self.recusar(format!("{caminho}: sumiu antes de virar conflito"));
            return self.concluir(chave, None, None);
        };
        cofre::gravar_unico(&self.opcoes.cofre, &nome, &atual).map_err(io)?;
        self.rel.conflitos += 1;
        self.concluir(chave, None, None)
    }
}

fn item_de(id: &str, c: &Conflito409) -> ItemRemoto {
    ItemRemoto {
        id: id.to_string(),
        versao: c.versao_atual,
        hash_blob: c.hash_blob.clone(),
        apagado: c.apagado,
        seq: 0,
    }
}
