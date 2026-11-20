//! A sincronização do app: o estado que a tela mostra, a thread do ciclo e a ponta que a tela
//! chama. Sem Dioxus: tudo roda e é testado no Windows contra um `nota-servidor` local.
//!
//! Desenho (o mesmo do app de mesa, `src/sync/runner.rs`, trocando o egui por mensagens e
//! um número de versão que a tela consulta):
//! - uma **thread** dona do `Sincronizador` (e do banco redb dele) roda um ciclo por vez;
//!   a tela nunca espera por ela, então um servidor fora do ar não trava nada;
//! - quem decide **quando** rodar é a `Agenda` (ao abrir, depois de salvar, ao voltar ao app,
//!   botão, nova tentativa);
//! - o `Remoto` vai embrulhado em `Gated`: o ciclo só segura o portão de gravação nos trechos
//!   de disco (renomear, salvar e o ciclo se revezam);
//! - **a chave do cofre (K) vive só na memória** (a frase nunca é guardada): ao abrir o app a
//!   sincronização fica em `Fase::PedeFrase` até o dono digitá-la.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use nota_cripto::ChaveCofre;
use nota_sync::caminhos::eh_temporario;
use nota_sync::remoto::{ErroRemoto, Remoto};
use nota_sync::{Config, ErroSync, Opcoes, RemotoHttp, Sincronizador};

use super::agenda::{Agenda, Tempos};
use super::ajustes::{APARELHO_PADRAO, Ajustes, Arquivos};
use super::cofre::Cofre;
use super::conexao::{ErroConexao, Resultado, estabelecer_chave};
use super::portao::{Gated, Portao};

// ------------------------------------------------------------------ o que a tela vê

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Fase {
    /// Nada configurado: a sincronização está desligada.
    #[default]
    SemConfig,
    /// Está conectado, mas a frase desta abertura do app ainda não foi digitada.
    PedeFrase,
    /// Falando com o servidor para abrir a chave.
    Conectando,
    Sincronizando,
    Sincronizado,
    /// O servidor não respondeu. O cofre funciona como sempre; o ciclo tenta de novo.
    SemConexao,
    /// O ciclo parou numa coisa que tentar de novo sozinho não resolve.
    Falhou,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EstadoSync {
    pub fase: Fase,
    /// Quando o último ciclo que chegou ao servidor terminou (hora local).
    pub ultima: Option<NaiveDateTime>,
    /// Arquivos `... (conflito <aparelho> <data>).md` no cofre.
    pub conflitos: Vec<String>,
    /// O que o último ciclo deixou de fora e por quê.
    pub recusados: Vec<String>,
    /// A frase em linguagem simples do que deu errado (ciclo ou conexão).
    pub erro: Option<String>,
    /// Por que o servidor não respondeu (para `SemConexao`).
    pub motivo_rede: Option<String>,
    /// `(apagados, arquivos)` de um ciclo parado pela guarda contra exclusão em massa.
    pub em_massa: Option<(usize, usize)>,
    /// Ciclos terminados desde que o app abriu.
    pub ciclos: u64,
    /// Quantos ciclos deixaram os arquivos do cofre diferentes (a tela se refaz).
    pub cofre_mudou: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tom {
    Neutro,
    Bom,
    Ocupado,
    Atencao,
    Ruim,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rotulo {
    pub texto: String,
    pub tom: Tom,
}

impl EstadoSync {
    /// Uma frase curta para o topo da lista e da tela.
    pub fn rotulo(&self) -> Rotulo {
        let (texto, tom) = match self.fase {
            Fase::SemConfig => ("Sincronização desligada".to_string(), Tom::Neutro),
            Fase::PedeFrase => ("Digite a frase para sincronizar".to_string(), Tom::Atencao),
            Fase::Conectando | Fase::Sincronizando => ("Sincronizando…".to_string(), Tom::Ocupado),
            Fase::Falhou => ("Erro de sincronização".to_string(), Tom::Ruim),
            Fase::SemConexao => ("Sem conexão".to_string(), Tom::Atencao),
            Fase::Sincronizado if !self.conflitos.is_empty() => (
                format!(
                    "{} conflito{}",
                    self.conflitos.len(),
                    if self.conflitos.len() == 1 { "" } else { "s" }
                ),
                Tom::Atencao,
            ),
            Fase::Sincronizado => (
                match self.ultima {
                    Some(t) => format!("Sincronizado {}", t.format("%H:%M")),
                    None => "Sincronizado".to_string(),
                },
                Tom::Bom,
            ),
        };
        Rotulo { texto, tom }
    }

    /// "20/11 14:32", ou "hoje 14:32" se for do dia de `agora`.
    pub fn ultima_em_texto(&self, agora: NaiveDateTime) -> Option<String> {
        let t = self.ultima?;
        Some(if t.date() == agora.date() {
            format!("hoje {}", t.format("%H:%M"))
        } else {
            t.format("%d/%m %H:%M").to_string()
        })
    }

    /// O texto da parada por exclusão em massa (para a tela explicar e oferecer "Permitir").
    pub fn texto_em_massa(&self) -> Option<String> {
        self.em_massa.map(|(apagados, total)| {
            format!(
                "Esta sincronização apagaria {apagados} de {total} arquivos do servidor. O cofre \
                 parece vazio ou a pasta não está acessível, então nada foi enviado. Se você \
                 apagou de propósito, permita uma vez."
            )
        })
    }
}

/// Traduz o erro do ciclo para a fase e a frase que a tela mostra.
pub fn explicar_erro(e: &ErroSync) -> (Fase, String, Option<String>) {
    match e {
        ErroSync::Remoto(ErroRemoto::Rede(motivo)) => (
            Fase::SemConexao,
            "Não consegui falar com o servidor. Confira se a Tailscale está ligada neste celular \
             (ou se você está na rede de casa). O app tenta de novo daqui a pouco e sempre que \
             você salvar uma nota ou voltar ao app."
                .to_string(),
            Some(motivo.clone()),
        ),
        ErroSync::Remoto(ErroRemoto::NaoAutorizado) => (
            Fase::Falhou,
            "O servidor recusou o token deste aparelho (401). Desconecte e conecte de novo com um \
             token válido."
                .to_string(),
            None,
        ),
        ErroSync::Remoto(ErroRemoto::NaoEncontrado) => (
            Fase::Falhou,
            "O servidor não tem um arquivo que o índice dele cita (404). Sincronize de novo; se \
             continuar, avise quem cuida do servidor."
                .to_string(),
            None,
        ),
        ErroSync::Remoto(ErroRemoto::Protocolo(m)) => (
            Fase::Falhou,
            format!(
                "O servidor respondeu algo que o app não entendeu ({m}). Confira se o endereço é \
                 mesmo o do servidor de notas."
            ),
            None,
        ),
        ErroSync::Io(m) => (
            Fase::Falhou,
            format!("Problema no armazenamento do celular: {m}"),
            None,
        ),
        ErroSync::Estado(m) => (
            Fase::Falhou,
            format!(
                "Problema no controle interno da sincronização ({m}). Desconectar e conectar de \
                 novo recomeça esse controle sem perder nenhuma nota."
            ),
            None,
        ),
        ErroSync::ExclusaoEmMassa { .. } => (Fase::Falhou, e.to_string(), None),
        ErroSync::Interrompido => (Fase::Falhou, "Sincronização interrompida.".into(), None),
    }
}

// ------------------------------------------------------------------ estado compartilhado

/// O estado e o aviso de "mudou", compartilhados entre a thread do ciclo e a tela.
struct Placar {
    estado: Mutex<EstadoSync>,
    mudou: Condvar,
    versao: AtomicU64,
}

impl Placar {
    fn novo(fase: Fase) -> Self {
        Self {
            estado: Mutex::new(EstadoSync {
                fase,
                ..EstadoSync::default()
            }),
            mudou: Condvar::new(),
            versao: AtomicU64::new(1),
        }
    }

    fn trava(&self) -> MutexGuard<'_, EstadoSync> {
        self.estado.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn atualizar(&self, f: impl FnOnce(&mut EstadoSync)) {
        f(&mut self.trava());
        self.versao.fetch_add(1, Ordering::SeqCst);
        self.mudou.notify_all();
    }
}

enum Comando {
    Agora,
    Gravou,
    Voltou,
    PermitirMassaUmaVez,
    Parar,
}

struct Robo {
    comandos: mpsc::Sender<Comando>,
    terminou: mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl Robo {
    /// Pede para parar e espera até `espera`. Um ciclo preso num servidor morto não é esperado
    /// (a thread termina sozinha quando a chamada estoura o tempo).
    fn parar(mut self, espera: Duration) -> bool {
        let _ = self.comandos.send(Comando::Parar);
        let acabou = self.terminou.recv_timeout(espera).is_ok();
        if acabou && let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        acabou
    }
}

impl Drop for Robo {
    fn drop(&mut self) {
        let _ = self.comandos.send(Comando::Parar);
    }
}

/// O que o dono digita na tela.
#[derive(Clone)]
pub struct Entrada {
    pub servidor: String,
    pub aparelho: String,
    pub token: String,
    pub frase: String,
}

impl std::fmt::Debug for Entrada {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entrada")
            .field("servidor", &self.servidor)
            .field("aparelho", &self.aparelho)
            .field("token", &"***")
            .field("frase", &"***")
            .finish()
    }
}

struct Interno {
    cofre: Cofre,
    arquivos: Arquivos,
    tempos: Tempos,
    placar: Arc<Placar>,
    robo: Mutex<Option<Robo>>,
    /// Quantas gravações do app a agenda já viu.
    gravacoes_vistas: AtomicU64,
}

/// A ponta que a tela usa. Barata de clonar; igual a outra se for a mesma sincronização.
#[derive(Clone)]
pub struct Sincronia(Arc<Interno>);

impl PartialEq for Sincronia {
    fn eq(&self, outra: &Self) -> bool {
        Arc::ptr_eq(&self.0, &outra.0)
    }
}

impl Eq for Sincronia {}

impl std::fmt::Debug for Sincronia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sincronia").finish_non_exhaustive()
    }
}

/// Quanto esperar a thread do ciclo antes de seguir sem ela.
const ESPERA_PARAR: Duration = Duration::from_secs(8);

impl Sincronia {
    /// A sincronização de um cofre. Não toca a rede: só lê a configuração salva (conectado =
    /// espera a frase) e passa a usar o portão e o nome do aparelho dela.
    pub fn nova(cofre: Cofre, arquivos: Arquivos, tempos: Tempos) -> Self {
        let (fase, erro) = match arquivos.carregar() {
            Ok(a) => {
                if !a.aparelho.trim().is_empty() {
                    cofre.portao().0.definir_aparelho(&a.aparelho);
                }
                (
                    if a.conectado {
                        Fase::PedeFrase
                    } else {
                        Fase::SemConfig
                    },
                    None,
                )
            }
            Err(e) => (Fase::SemConfig, Some(e)),
        };
        let placar = Placar::novo(fase);
        placar.trava().erro = erro;
        let vistas = cofre.portao().0.gravacoes();
        Self(Arc::new(Interno {
            cofre,
            arquivos,
            tempos,
            placar: Arc::new(placar),
            robo: Mutex::new(None),
            gravacoes_vistas: AtomicU64::new(vistas),
        }))
    }

    pub fn estado(&self) -> EstadoSync {
        self.0.placar.trava().clone()
    }

    /// Sobe a cada mudança do estado: a tela compara com a última que viu.
    pub fn versao(&self) -> u64 {
        self.0.placar.versao.load(Ordering::SeqCst)
    }

    /// As configurações salvas (o token inclusive; a tela só mostra o endereço e o nome).
    pub fn ajustes(&self) -> Ajustes {
        self.0.arquivos.carregar().unwrap_or_default()
    }

    /// Põe o estado que a tela vai mostrar (os prints fotografam cada caso sem servidor).
    #[cfg(test)]
    pub fn definir_estado_para_teste(&self, estado: EstadoSync) {
        self.0.placar.atualizar(|s| *s = estado);
    }

    // ---- conectar

    /// Conecta pela primeira vez (ou de novo, com outro servidor): abre a chave com a frase,
    /// guarda endereço, nome e token, recomeça o controle interno e liga o ciclo. **Bloqueia na
    /// rede**: a tela chama `conectar_em_segundo_plano`.
    pub fn conectar(&self, entrada: Entrada) -> Result<Resultado, ErroConexao> {
        self.abrir_sessao(entrada, true)
    }

    /// Com a configuração já salva, só falta a frase desta abertura do app. Bloqueia na rede.
    pub fn entrar(&self, frase: &str) -> Result<Resultado, ErroConexao> {
        let a = self.0.arquivos.carregar().map_err(ErroConexao::Local)?;
        if !a.conectado {
            return Err(ErroConexao::Ajustes(
                "Este aparelho ainda não está conectado a um servidor.".into(),
            ));
        }
        self.abrir_sessao(
            Entrada {
                servidor: a.servidor,
                aparelho: a.aparelho,
                token: a.token,
                frase: frase.to_string(),
            },
            false,
        )
    }

    pub fn conectar_em_segundo_plano(&self, entrada: Entrada) {
        let s = self.clone();
        self.em_thread("rustidian-conectar", move || {
            let _ = s.conectar(entrada);
        });
    }

    pub fn entrar_em_segundo_plano(&self, frase: String) {
        let s = self.clone();
        self.em_thread("rustidian-entrar", move || {
            let _ = s.entrar(&frase);
        });
    }

    fn em_thread(&self, nome: &str, corpo: impl FnOnce() + Send + 'static) {
        if let Err(e) = std::thread::Builder::new().name(nome.into()).spawn(corpo) {
            self.0.placar.atualizar(|s| {
                s.erro = Some(format!("Não consegui iniciar a conexão: {e}"));
            });
        }
    }

    fn abrir_sessao(&self, entrada: Entrada, nova: bool) -> Result<Resultado, ErroConexao> {
        let antes = self.0.placar.trava().fase;
        // Quando a tentativa falha, volta para onde estava (a frase errada não "desconecta").
        let rodando = self
            .0
            .robo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some();
        let voltar_para = if rodando {
            if antes == Fase::Conectando {
                Fase::Sincronizado
            } else {
                antes
            }
        } else if self.ajustes().conectado {
            Fase::PedeFrase
        } else {
            Fase::SemConfig
        };
        self.0.placar.atualizar(|s| {
            s.fase = Fase::Conectando;
            s.erro = None;
        });
        match self.preparar(&entrada) {
            Err(e) => {
                self.0.placar.atualizar(|s| {
                    s.fase = voltar_para;
                    s.erro = Some(e.to_string());
                });
                Err(e)
            }
            Ok((chave, remoto, ajustes, resultado)) => {
                self.parar_robo();
                if nova {
                    self.0.arquivos.esquecer_banco();
                    self.0.arquivos.salvar(&ajustes).map_err(|m| {
                        let e = ErroConexao::Local(m);
                        self.0.placar.atualizar(|s| {
                            s.fase = voltar_para;
                            s.erro = Some(e.to_string());
                        });
                        e
                    })?;
                }
                self.0.cofre.portao().0.definir_aparelho(&ajustes.aparelho);
                self.ligar(chave, remoto, ajustes.aparelho);
                Ok(resultado)
            }
        }
    }

    /// Confere os campos, fala com o servidor e abre a chave. Não grava nada no disco.
    fn preparar(
        &self,
        entrada: &Entrada,
    ) -> Result<(ChaveCofre, RemotoHttp, Ajustes, Resultado), ErroConexao> {
        let aparelho = if entrada.aparelho.trim().is_empty() {
            APARELHO_PADRAO.to_string()
        } else {
            entrada.aparelho.trim().to_string()
        };
        let config = Config {
            servidor: entrada.servidor.trim().to_string(),
            token: entrada.token.trim().to_string(),
            aparelho: aparelho.clone(),
        };
        config.validar().map_err(ErroConexao::Ajustes)?;
        let mut remoto = RemotoHttp::novo(&config).map_err(ErroConexao::Ajustes)?;
        let (chave, resultado) = estabelecer_chave(&mut remoto, &entrada.frase)?;
        let servidor =
            nota_sync::http::base_do_servidor(&config.servidor).map_err(ErroConexao::Ajustes)?;
        Ok((
            chave,
            remoto,
            Ajustes {
                servidor,
                aparelho,
                token: config.token,
                conectado: true,
            },
            resultado,
        ))
    }

    /// Esquece a conexão: para o ciclo, apaga o token e o controle interno. As notas, o
    /// servidor e a lixeira ficam como estão; endereço e nome do aparelho ficam para reconectar.
    pub fn desconectar(&self) -> Result<(), String> {
        self.parar_robo();
        self.0.arquivos.esquecer_banco();
        let mut a = self.0.arquivos.carregar().unwrap_or_default();
        a.conectado = false;
        a.token.clear();
        self.0.arquivos.salvar(&a)?;
        self.0.placar.atualizar(|s| {
            *s = EstadoSync {
                ciclos: s.ciclos,
                cofre_mudou: s.cofre_mudou,
                conflitos: s.conflitos.clone(),
                ..EstadoSync::default()
            };
        });
        Ok(())
    }

    // ---- gatilhos (nunca bloqueiam; sem ciclo ligado não fazem nada)

    fn enviar(&self, c: Comando) {
        if let Some(r) = &*self.0.robo.lock().unwrap_or_else(PoisonError::into_inner) {
            let _ = r.comandos.send(c);
        }
    }

    /// "Sincronizar agora".
    pub fn agora(&self) {
        self.enviar(Comando::Agora);
    }

    /// O app voltou à tela.
    pub fn voltou(&self) {
        self.enviar(Comando::Voltou);
    }

    /// O dono permitiu, uma vez, o ciclo que parou na guarda de exclusão em massa.
    pub fn permitir_exclusao_em_massa_uma_vez(&self) {
        self.enviar(Comando::PermitirMassaUmaVez);
    }

    /// Confere se o app gravou algo no cofre desde a última vez e, se sim, agenda um ciclo
    /// (com a folga de `Tempos::depois_de_salvar`). Barato: a tela chama a cada tique.
    pub fn observar_gravacoes(&self) {
        let agora = self.0.cofre.portao().0.gravacoes();
        let antes = self.0.gravacoes_vistas.swap(agora, Ordering::SeqCst);
        if agora > antes {
            self.enviar(Comando::Gravou);
        }
    }

    /// Espera um ciclo terminar depois do de número `depois` (ou `limite`). Para testes.
    #[cfg(test)]
    pub fn esperar_ciclo(&self, depois: u64, limite: Duration) -> Option<EstadoSync> {
        let fim = Instant::now() + limite;
        let mut s = self.0.placar.trava();
        loop {
            if s.ciclos > depois && s.fase != Fase::Sincronizando && s.fase != Fase::Conectando {
                return Some(s.clone());
            }
            let falta = fim.checked_duration_since(Instant::now())?;
            s = self
                .0
                .placar
                .mudou
                .wait_timeout(s, falta)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Para o ciclo (os testes terminaram, ou simulam fechar o app).
    #[cfg(test)]
    pub fn parar(&self) {
        self.parar_robo();
    }

    fn parar_robo(&self) {
        let robo = self
            .0
            .robo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(r) = robo {
            r.parar(ESPERA_PARAR);
        }
    }

    // ---- a thread

    fn ligar<R: Remoto + Send + 'static>(&self, chave: ChaveCofre, remoto: R, aparelho: String) {
        let (comandos, receptor) = mpsc::channel();
        let (fim, terminou) = mpsc::channel();
        let placar = Arc::clone(&self.0.placar);
        let portao = Arc::clone(&self.0.cofre.portao().0);
        let opcoes = Opcoes {
            cofre: self.0.cofre.raiz().to_path_buf(),
            estado: self.0.arquivos.banco(),
            aparelho,
        };
        let pasta = self.0.arquivos.pasta().to_path_buf();
        let tempos = self.0.tempos;
        let thread = std::thread::Builder::new()
            .name("rustidian-sync".into())
            .spawn(move || {
                let _ = std::fs::create_dir_all(&pasta);
                rodar(opcoes, chave, remoto, portao, tempos, &receptor, &placar);
                let _ = fim.send(());
            });
        match thread {
            Ok(t) => {
                // O primeiro ciclo ("ao abrir") já está pedido pela `Agenda`.
                self.0.placar.atualizar(|s| {
                    s.fase = Fase::Sincronizando;
                    s.erro = None;
                    s.motivo_rede = None;
                    s.em_massa = None;
                });
                *self.0.robo.lock().unwrap_or_else(PoisonError::into_inner) = Some(Robo {
                    comandos,
                    terminou,
                    thread: Some(t),
                });
            }
            Err(e) => self.0.placar.atualizar(|s| {
                s.fase = Fase::Falhou;
                s.erro = Some(format!("Não consegui iniciar a sincronização: {e}"));
            }),
        }
    }
}

/// O corpo da thread.
fn rodar<R: Remoto + Send + 'static>(
    opcoes: Opcoes,
    chave: ChaveCofre,
    remoto: R,
    portao: Arc<Portao>,
    tempos: Tempos,
    comandos: &mpsc::Receiver<Comando>,
    placar: &Placar,
) {
    let cofre = opcoes.cofre.clone();
    let mut sinc = match Sincronizador::novo(opcoes, chave, Gated::novo(remoto, portao)) {
        Ok(s) => s,
        Err(e) => {
            placar.atualizar(|s| {
                s.fase = Fase::Falhou;
                s.erro = Some(format!(
                    "Não consegui iniciar a sincronização: {e}. Desconectar e conectar de novo \
                     costuma resolver."
                ));
            });
            return;
        }
    };
    let conflitos = varrer_cofre(&cofre).conflitos;
    placar.atualizar(|s| s.conflitos = conflitos);

    let mut agenda = Agenda::nova(tempos);
    let mut permitir_massa = false;
    loop {
        let agora = Instant::now();
        let espera = agenda
            .proximo(agora)
            .map_or(Duration::from_secs(3600), |t| {
                t.saturating_duration_since(agora)
            });
        let mut fila = Vec::new();
        match comandos.recv_timeout(espera) {
            Ok(c) => fila.push(c),
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        // Tudo que já chegou entra na agenda antes de decidir: dez toques em "Sincronizar
        // agora" viram um ciclo, não dez.
        fila.extend(comandos.try_iter());
        for c in fila {
            match c {
                Comando::Parar => return,
                Comando::Agora => agenda.pedir_agora(),
                Comando::Gravou => agenda.gravou(Instant::now()),
                Comando::Voltou => agenda.voltou(Instant::now()),
                Comando::PermitirMassaUmaVez => {
                    permitir_massa = true;
                    agenda.pedir_agora();
                }
            }
        }
        if !agenda.devido(Instant::now()) {
            continue;
        }
        agenda.comecou(Instant::now());
        let fim = um_ciclo(&mut sinc, &cofre, placar, permitir_massa);
        permitir_massa = false;
        agenda.terminou(Instant::now(), fim == Fim::Rede);
        if fim == Fim::Panico {
            return;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fim {
    Bom,
    Rede,
    Falha,
    Panico,
}

/// Um ciclo. O portão é preso só nos trechos de disco (o `Gated` o solta nas chamadas de rede).
fn um_ciclo<R: Remoto>(
    sinc: &mut Sincronizador<Gated<R>>,
    cofre: &Path,
    placar: &Placar,
    permitir_massa: bool,
) -> Fim {
    placar.atualizar(|s| s.fase = Fase::Sincronizando);
    let antes = varrer_cofre(cofre);
    sinc.permitir_exclusao_em_massa(permitir_massa);
    sinc.remoto().entrar();
    let resultado = std::panic::catch_unwind(AssertUnwindSafe(|| sinc.ciclo()));
    sinc.remoto().sair();
    sinc.permitir_exclusao_em_massa(false);
    let depois = varrer_cofre(cofre);
    let mudou = depois.impressao != antes.impressao;
    let agora = chrono::Local::now().naive_local();
    let mut fim = Fim::Bom;
    placar.atualizar(|s| {
        s.ciclos += 1;
        s.conflitos = depois.conflitos;
        if mudou {
            s.cofre_mudou += 1;
        }
        s.motivo_rede = None;
        match resultado {
            Ok(Ok(rel)) => {
                s.fase = Fase::Sincronizado;
                s.ultima = Some(agora);
                s.erro = None;
                s.em_massa = None;
                s.recusados = rel.recusados;
            }
            Ok(Err(e)) => {
                let (fase, texto, motivo) = explicar_erro(&e);
                s.fase = fase;
                s.erro = Some(texto);
                s.motivo_rede = motivo;
                s.em_massa = match e {
                    ErroSync::ExclusaoEmMassa { lapides, total } => Some((lapides, total)),
                    _ => None,
                };
                fim = if fase == Fase::SemConexao {
                    Fim::Rede
                } else {
                    Fim::Falha
                };
            }
            Err(_) => {
                s.fase = Fase::Falhou;
                s.erro = Some(
                    "O mecanismo de sincronização parou de forma inesperada. Feche e abra o app."
                        .into(),
                );
                fim = Fim::Panico;
            }
        }
    });
    fim
}

// ------------------------------------------------------------------ olhar o cofre

/// O que uma volta barata pelo cofre acha: as cópias de conflito e uma impressão digital dos
/// arquivos (nomes, tamanhos e datas; nada é lido), para saber se o ciclo mudou alguma coisa.
pub struct Varredura {
    pub conflitos: Vec<String>,
    pub impressao: u64,
}

pub fn varrer_cofre(raiz: &Path) -> Varredura {
    let mut v = Varredura {
        conflitos: Vec::new(),
        impressao: 0,
    };
    andar(raiz, "", &mut v);
    v.conflitos.sort();
    v
}

fn andar(raiz: &Path, relativo: &str, v: &mut Varredura) {
    let pasta = if relativo.is_empty() {
        raiz.to_path_buf()
    } else {
        raiz.join(relativo)
    };
    let Ok(entradas) = std::fs::read_dir(pasta) else {
        return;
    };
    for e in entradas.flatten() {
        let Some(nome) = e.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // `.trash`, `.obsidian`, `.git` e os temporários do motor ficam de fora.
        if nome.starts_with('.') || eh_temporario(&nome) {
            continue;
        }
        let Ok(tipo) = e.file_type() else {
            continue;
        };
        let caminho = if relativo.is_empty() {
            nome.clone()
        } else {
            format!("{relativo}/{nome}")
        };
        if tipo.is_dir() {
            andar(raiz, &caminho, v);
        } else if tipo.is_file() {
            if eh_copia_de_conflito(&nome) {
                v.conflitos.push(caminho.clone());
            }
            let mut h = DefaultHasher::new();
            caminho.hash(&mut h);
            if let Ok(m) = e.metadata() {
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
            }
            v.impressao = v.impressao.wrapping_add(h.finish());
        }
    }
}

/// `nota (conflito <aparelho> <data>).md`, como o `nota-sync` (e o editor) nomeiam as cópias.
pub fn eh_copia_de_conflito(nome: &str) -> bool {
    // O `nota-sync` pode acrescentar " 2", " 3" depois do parêntese se o nome estava tomado.
    nome.find(" (conflito ")
        .is_some_and(|i| nome[i..].contains(')'))
}

#[cfg(test)]
mod testes {
    use super::*;
    use chrono::NaiveDate;

    fn as_(h: u32, m: u32) -> Option<NaiveDateTime> {
        NaiveDate::from_ymd_opt(2026, 11, 20)
            .unwrap()
            .and_hms_opt(h, m, 0)
    }

    #[test]
    fn rotulos_seguem_a_prioridade() {
        let mut s = EstadoSync::default();
        assert_eq!(s.rotulo().texto, "Sincronização desligada");
        s.fase = Fase::PedeFrase;
        assert_eq!(s.rotulo().tom, Tom::Atencao);
        s.fase = Fase::Sincronizando;
        assert_eq!(s.rotulo().texto, "Sincronizando…");
        s.fase = Fase::Sincronizado;
        s.ultima = as_(9, 5);
        assert_eq!(s.rotulo().texto, "Sincronizado 09:05");
        assert_eq!(s.rotulo().tom, Tom::Bom);
        s.conflitos = vec!["a (conflito pc 2026-11-20 0905).md".into()];
        assert_eq!(s.rotulo().texto, "1 conflito");
        s.conflitos
            .push("b (conflito pc 2026-11-20 0905).md".into());
        assert_eq!(s.rotulo().texto, "2 conflitos");
        s.fase = Fase::SemConexao;
        assert_eq!(s.rotulo().texto, "Sem conexão");
        s.fase = Fase::Falhou;
        assert_eq!(s.rotulo().texto, "Erro de sincronização");
        assert_eq!(s.rotulo().tom, Tom::Ruim);
    }

    #[test]
    fn data_da_ultima_sincronizacao_em_texto() {
        let mut s = EstadoSync::default();
        let agora = as_(18, 0).unwrap();
        assert_eq!(s.ultima_em_texto(agora), None);
        s.ultima = as_(14, 32);
        assert_eq!(s.ultima_em_texto(agora).as_deref(), Some("hoje 14:32"));
        s.ultima = NaiveDate::from_ymd_opt(2026, 11, 19)
            .unwrap()
            .and_hms_opt(8, 1, 0);
        assert_eq!(s.ultima_em_texto(agora).as_deref(), Some("19/11 08:01"));
    }

    #[test]
    fn erros_viram_frases_simples() {
        let (fase, texto, motivo) =
            explicar_erro(&ErroSync::Remoto(ErroRemoto::Rede("timeout".into())));
        assert_eq!(fase, Fase::SemConexao);
        assert!(texto.contains("Tailscale"));
        assert_eq!(motivo.as_deref(), Some("timeout"));

        let (fase, texto, _) = explicar_erro(&ErroSync::Remoto(ErroRemoto::NaoAutorizado));
        assert_eq!(fase, Fase::Falhou);
        assert!(texto.contains("token"));

        let (fase, texto, _) = explicar_erro(&ErroSync::ExclusaoEmMassa {
            lapides: 25,
            total: 30,
        });
        assert_eq!(fase, Fase::Falhou);
        assert!(texto.contains("25 de 30"));

        for e in [
            ErroSync::Io("x".into()),
            ErroSync::Estado("y".into()),
            ErroSync::Remoto(ErroRemoto::Protocolo("z".into())),
            ErroSync::Remoto(ErroRemoto::NaoEncontrado),
        ] {
            let (fase, texto, _) = explicar_erro(&e);
            assert_eq!(fase, Fase::Falhou);
            assert!(!texto.is_empty());
        }
    }

    #[test]
    fn texto_de_exclusao_em_massa() {
        let s = EstadoSync {
            em_massa: Some((25, 30)),
            ..EstadoSync::default()
        };
        let t = s.texto_em_massa().unwrap();
        assert!(t.contains("25 de 30") && t.contains("nada foi enviado"));
        assert!(EstadoSync::default().texto_em_massa().is_none());
    }

    #[test]
    fn copias_de_conflito_pelo_nome() {
        assert!(eh_copia_de_conflito(
            "Nota (conflito pc 2026-11-20 0905).md"
        ));
        assert!(eh_copia_de_conflito(
            "Nota (conflito pc 2026-11-20 0905) 2.md"
        ));
        assert!(!eh_copia_de_conflito("Nota.md"));
        assert!(!eh_copia_de_conflito("Conversa sobre (conflito).md"));
        assert!(!eh_copia_de_conflito("Nota (conflict).md"));
    }

    #[test]
    fn a_volta_pelo_cofre_acha_conflitos_e_pula_lixeira_e_temporarios() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("a/b")).unwrap();
        std::fs::create_dir_all(t.path().join(".trash")).unwrap();
        std::fs::write(t.path().join("a/b/x (conflito pc 2026-11-20 0905).md"), "1").unwrap();
        std::fs::write(
            t.path().join(".trash/y (conflito pc 2026-11-20 0905).md"),
            "1",
        )
        .unwrap();
        std::fs::write(t.path().join("simples.md"), "1").unwrap();
        let v1 = varrer_cofre(t.path());
        assert_eq!(v1.conflitos, ["a/b/x (conflito pc 2026-11-20 0905).md"]);
        std::fs::write(t.path().join("simples.md"), "12").unwrap();
        assert_ne!(varrer_cofre(t.path()).impressao, v1.impressao);
        // Um temporário do motor não mexe na impressão.
        let antes = varrer_cofre(t.path()).impressao;
        std::fs::write(t.path().join(".nota-sync-ab.tmp"), "x").unwrap();
        assert_eq!(varrer_cofre(t.path()).impressao, antes);
    }

    #[test]
    fn entrada_nao_mostra_token_nem_frase_no_debug() {
        let e = Entrada {
            servidor: "http://x".into(),
            aparelho: "a".into(),
            token: "token-secreto".into(),
            frase: "frase-secreta".into(),
        };
        let d = format!("{e:?}");
        assert!(!d.contains("token-secreto") && !d.contains("frase-secreta"));
    }

    #[test]
    fn sem_configuracao_a_fase_inicial_e_sem_config_e_com_conexao_pede_a_frase() {
        let t = tempfile::tempdir().unwrap();
        let cofre = Cofre::abrir(t.path().join("Cofre")).unwrap();
        let arquivos = Arquivos::em(t.path().join("sync"));
        let s = Sincronia::nova(cofre.clone(), arquivos.clone(), Tempos::default());
        assert_eq!(s.estado().fase, Fase::SemConfig);
        arquivos
            .salvar(&Ajustes {
                servidor: "http://100.1.2.3:8443".into(),
                aparelho: "Pixel".into(),
                token: "t".into(),
                conectado: true,
            })
            .unwrap();
        let s = Sincronia::nova(cofre.clone(), arquivos, Tempos::default());
        assert_eq!(s.estado().fase, Fase::PedeFrase);
        // O nome do aparelho vai para o portão (nome das cópias de conflito do editor).
        assert_eq!(cofre.portao().0.aparelho(), "Pixel");
    }

    #[test]
    fn observar_gravacoes_so_agenda_quando_o_app_gravou() {
        let t = tempfile::tempdir().unwrap();
        let cofre = Cofre::abrir(t.path().join("Cofre")).unwrap();
        let s = Sincronia::nova(
            cofre.clone(),
            Arquivos::em(t.path().join("sync")),
            Tempos::default(),
        );
        // Sem ciclo ligado nada acontece (e nada quebra).
        s.observar_gravacoes();
        cofre.salvar_nota("a.md", "x").unwrap();
        let antes = s.0.gravacoes_vistas.load(Ordering::SeqCst);
        s.observar_gravacoes();
        assert!(s.0.gravacoes_vistas.load(Ordering::SeqCst) > antes);
    }
}
