//! O banco do histórico: versões de cada nota, comprimidas e abertas na hora.
//!
//! # Como as versões ficam guardadas
//!
//! Cada nota tem uma **cadeia** de versões numeradas (1, 2, 3...; o número nunca é reaproveitado):
//!
//! - A **mais nova** é sempre uma cópia cheia ("quadro-chave"): abrir a nota de hoje não paga
//!   reconstrução.
//! - Cada versão anterior vira uma **diferença para trás**: o zstd comprime o texto dela sabendo
//!   da vizinha mais nova (ver [`crate::codec`]). Uma edição de ~50 caracteres vira ~45 bytes.
//! - A cada N versões (padrão 128, [`Opcoes::quadro_chave_a_cada`]) uma versão fica como
//!   quadro-chave em vez de virar diferença, para limitar quantas diferenças é preciso aplicar
//!   ao abrir uma versão antiga (no máximo N).
//! - Os quadros-chave não são um bloco só: o texto é cortado em **pedaços** de ~2 KB pelo próprio
//!   conteúdo ([`crate::pedacos`]), cada pedaço comprimido e guardado **uma vez** pela chave
//!   blake3. Quadros-chave parecidos (da mesma nota ou de notas diferentes, como um parágrafo
//!   copiado) repartem os pedaços. Uma contagem de referências apaga o pedaço quando ninguém
//!   mais o usa.
//! - Cada versão guarda o blake3 do texto inteiro (128 bits, [`crate::registro::Impressao`]); [`Historico::ler`] confere e devolve
//!   [`Erro::Integridade`] se não bater.
//!
//! # Tabelas do `redb`
//!
//! | Tabela     | Chave                     | Valor                                          |
//! |------------|---------------------------|------------------------------------------------|
//! | `meta`     | texto                     | formato e nome do codec (banco de outro codec não abre) |
//! | `notas`    | `NotaId` (texto)          | número curto da nota (chave fixa e pequena nas versões) |
//! | `versoes`  | (número da nota, versão)  | registro binário ([`crate::registro`])         |
//! | `pedacos`  | blake3 do pedaço          | tamanho original + zstd                        |
//! | `refs`     | blake3 do pedaço          | contagem de referências                        |
//!
//! # Durabilidade
//!
//! Com [`Durabilidade::Imediata`] cada gravação espera o disco (segura, mas um `fsync` por versão
//! custa milissegundos). Com [`Durabilidade::Adiada`] a gravação só vai ao disco no próximo
//! [`Historico::persistir`] (o editor chama de tempos em tempos e ao fechar; `Drop` também chama):
//! uma queda de energia perde só as últimas versões, nunca corrompe o banco. O `.md` continua
//! sendo a verdade; o histórico é um anexo.

use crate::codec::{Codec, ZstdCodec};
use crate::erro::{Erro, Result};
use crate::pedacos::{Chave, ParametrosPedacos, chave, cortar};
use crate::registro::{
    Corpo, Impressao, NotaId, Origem, Registro, VersaoId, VersaoInfo, impressao_de_bytes, ms_de,
};
use crate::retencao::{Candidata, PoliticaRetencao, escolher_remocoes};
use redb::{
    Database, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, Table,
    TableDefinition, WriteTransaction,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const NOTAS: TableDefinition<&str, u32> = TableDefinition::new("notas");
const VERSOES: TableDefinition<(u32, u64), &[u8]> = TableDefinition::new("versoes");
const PEDACOS: TableDefinition<[u8; 32], &[u8]> = TableDefinition::new("pedacos");
const REFS: TableDefinition<[u8; 32], u32> = TableDefinition::new("refs");

const FORMATO: u8 = 1;

type TVersoes<'t> = Table<'t, (u32, u64), &'static [u8]>;
type TPedacos<'t> = Table<'t, [u8; 32], &'static [u8]>;
type TRefs<'t> = Table<'t, [u8; 32], u32>;

/// Quando uma gravação é considerada segura em disco.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durabilidade {
    /// Cada gravação espera o `fsync`.
    Imediata,
    /// A gravação fica na memória do banco até [`Historico::persistir`] (ou o próximo `podar`,
    /// `compactar`, `nomear`...). Mais rápida; uma queda de energia perde as últimas versões.
    Adiada,
}

/// Ajustes do histórico.
#[derive(Debug, Clone, Copy)]
pub struct Opcoes {
    /// De quantas em quantas versões há um quadro-chave (limita as diferenças a aplicar ao abrir).
    ///
    /// Padrão 128: medido com a nota de 5 KB e 1.000 versões, 64 dá 204,8 KB de arquivo e 128 dá
    /// 172 KB, com a pior abertura abaixo de 1 ms nos dois (ver `README.md`). Cada quadro-chave
    /// guarda a nota inteira comprimida, então o espaço cai quase pela metade; em compensação, abrir
    /// aplica até N-1 diferenças, e cada uma carrega a versão vizinha inteira como base. Para notas
    /// muito maiores (dezenas de KB) vale um N menor, como 32 ou 64.
    pub quadro_chave_a_cada: u64,
    pub durabilidade: Durabilidade,
    pub pedacos: ParametrosPedacos,
}

impl Default for Opcoes {
    fn default() -> Self {
        Self {
            quadro_chave_a_cada: 128,
            durabilidade: Durabilidade::Adiada,
            pedacos: ParametrosPedacos::default(),
        }
    }
}

/// O que a poda fez.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelatorioPoda {
    pub versoes_removidas: usize,
    pub notas_afetadas: usize,
}

/// O que a compactação fez.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelatorioCompactacao {
    pub bytes_antes: u64,
    pub bytes_depois: u64,
}

/// O que apagar um trecho de todas as notas fez.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApagadoEmTodas {
    /// Em quantas notas o trecho aparecia.
    pub notas: usize,
    /// Em quantas versões (somando todas as notas).
    pub versoes: usize,
}

/// Quanto espaço uma nota usa.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EstatisticasNota {
    pub versoes: usize,
    /// Soma dos tamanhos dos textos de todas as versões (o que ocuparia sem compressão).
    pub bytes_texto: u64,
    /// Registros + diferenças + pedaços dos quadros-chave desta nota. Pedaços repartidos com outras
    /// notas ou entre quadros-chave são contados por inteiro em cada um: a soma das notas pode passar
    /// do total real ([`Estatisticas::tamanho_em_disco`]).
    pub bytes_guardados: u64,
}

/// Visão geral do banco.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Estatisticas {
    pub notas: usize,
    pub versoes: usize,
    /// Pedaços distintos guardados (depois da deduplicação).
    pub pedacos: usize,
    /// Bytes dos pedaços (comprimidos).
    pub bytes_pedacos: u64,
    /// Bytes dos registros de versão.
    pub bytes_registros: u64,
    /// Tamanho do arquivo `.redb`.
    pub tamanho_em_disco: u64,
}

/// O histórico de todas as notas de um cofre, num arquivo `redb`.
pub struct Historico {
    db: Database,
    caminho: PathBuf,
    codec: Box<dyn Codec>,
    opc: Opcoes,
    nao_persistido: bool,
}

fn u32_le(b: &[u8]) -> Result<u32> {
    b.get(..4)
        .map(|s| u32::from_le_bytes(s.try_into().expect("4 bytes")))
        .ok_or_else(|| Erro::Corrompido("pedaço sem cabeçalho".into()))
}

fn hash(b: &[u8]) -> Impressao {
    impressao_de_bytes(b)
}

fn texto_de(b: Vec<u8>) -> Result<String> {
    String::from_utf8(b).map_err(|_| Erro::Corrompido("versão não é UTF-8".into()))
}

/// Todas as versões de uma nota em ordem crescente.
fn todas<T: ReadableTable<(u32, u64), &'static [u8]>>(
    v: &T,
    num: u32,
) -> Result<Vec<(u64, Registro)>> {
    let mut saida = Vec::new();
    for r in v.range((num, 0)..=(num, u64::MAX))? {
        let (k, val) = r?;
        let id = k.value().1;
        saida.push((id, Registro::decodificar(id, val.value())?));
    }
    Ok(saida)
}

fn ultima<T: ReadableTable<(u32, u64), &'static [u8]>>(
    v: &T,
    num: u32,
) -> Result<Option<(u64, Registro)>> {
    match v.range((num, 0)..=(num, u64::MAX))?.next_back() {
        Some(r) => {
            let (k, val) = r?;
            let id = k.value().1;
            Ok(Some((id, Registro::decodificar(id, val.value())?)))
        }
        None => Ok(None),
    }
}

fn registro<T: ReadableTable<(u32, u64), &'static [u8]>>(
    v: &T,
    num: u32,
    id: u64,
) -> Result<Option<Registro>> {
    match v.get((num, id))? {
        Some(g) => Ok(Some(Registro::decodificar(id, g.value())?)),
        None => Ok(None),
    }
}

fn numero_da_nota<T: ReadableTable<&'static str, u32>>(
    n: &T,
    nota: &NotaId,
) -> Result<Option<u32>> {
    Ok(n.get(nota.como_str())?.map(|g| g.value()))
}

/// Reescreve o texto de uma versão: `None` = não muda.
type Troca<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Como a poda e a compactação mexem na cadeia de uma nota.
struct Mudancas<'a> {
    remover: &'a HashSet<u64>,
    /// Reescreve o texto de uma versão (devolve `None` se não muda).
    trocar: Option<Troca<'a>>,
    /// Ignora os quadros-chave que existem e refaz a distribuição (compactação).
    refazer: bool,
}

impl Historico {
    /// Abre (ou cria) o histórico com as opções e o codec padrão.
    pub fn abrir(caminho: &Path) -> Result<Historico> {
        Self::abrir_com(caminho, Opcoes::default(), Box::new(ZstdCodec::default()))
    }

    /// Abre (ou cria) o histórico com opções e codec à escolha. Um banco gravado com outro codec
    /// não abre: troque o codec por uma migração, não por engano.
    pub fn abrir_com(caminho: &Path, opc: Opcoes, codec: Box<dyn Codec>) -> Result<Historico> {
        let db = Database::create(caminho)?;
        let w = db.begin_write()?;
        {
            let mut meta = w.open_table(META)?;
            let codec_gravado = meta.get("codec")?.map(|g| g.value().to_vec());
            let formato = meta.get("formato")?.map(|g| g.value().to_vec());
            match (codec_gravado, formato) {
                (Some(c), Some(f)) => {
                    if f != [FORMATO] {
                        return Err(Erro::Corrompido(format!("formato {f:?} desconhecido")));
                    }
                    if c != codec.nome().as_bytes() {
                        return Err(Erro::Argumento(format!(
                            "o banco usa o codec {:?}, não {:?}",
                            String::from_utf8_lossy(&c),
                            codec.nome()
                        )));
                    }
                }
                _ => {
                    meta.insert("codec", codec.nome().as_bytes())?;
                    meta.insert("formato", [FORMATO].as_slice())?;
                }
            }
            w.open_table(NOTAS)?;
            w.open_table(VERSOES)?;
            w.open_table(PEDACOS)?;
            w.open_table(REFS)?;
        }
        w.commit()?;
        Ok(Historico {
            db,
            caminho: caminho.to_path_buf(),
            codec,
            opc,
            nao_persistido: false,
        })
    }

    fn txn_gravacao(&mut self, imediata: bool) -> Result<WriteTransaction> {
        let mut w = self.db.begin_write()?;
        if !imediata && self.opc.durabilidade == Durabilidade::Adiada {
            w.set_durability(Durability::None)?;
            self.nao_persistido = true;
        } else {
            w.set_durability(Durability::Immediate)?;
            self.nao_persistido = false;
        }
        Ok(w)
    }

    /// Garante em disco tudo o que foi gravado com [`Durabilidade::Adiada`].
    pub fn persistir(&mut self) -> Result<()> {
        let w = self.txn_gravacao(true)?;
        w.commit()?;
        self.nao_persistido = false;
        Ok(())
    }

    // ----------------------------------------------------------------- pedaços

    fn guardar_pedaco(
        &self,
        pedacos: &mut TPedacos,
        refs: &mut TRefs,
        c: &Chave,
        bytes: &[u8],
    ) -> Result<()> {
        let n = refs.get(c)?.map(|g| g.value());
        match n {
            Some(n) => {
                refs.insert(c, n + 1)?;
            }
            None => {
                let z = self.codec.comprimir(bytes)?;
                let mut v = Vec::with_capacity(4 + z.len());
                v.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                v.extend_from_slice(&z);
                pedacos.insert(c, v.as_slice())?;
                refs.insert(c, 1u32)?;
            }
        }
        Ok(())
    }

    fn soltar_pedaco(&self, pedacos: &mut TPedacos, refs: &mut TRefs, c: &Chave) -> Result<()> {
        let n = refs.get(c)?.map(|g| g.value());
        match n {
            Some(n) if n > 1 => {
                refs.insert(c, n - 1)?;
            }
            Some(_) => {
                refs.remove(c)?;
                pedacos.remove(c)?;
            }
            None => {}
        }
        Ok(())
    }

    fn texto_da_cheia<P: ReadableTable<[u8; 32], &'static [u8]>>(
        &self,
        pedacos: &P,
        chaves: &[Chave],
    ) -> Result<Vec<u8>> {
        let mut texto = Vec::new();
        for c in chaves {
            let g = pedacos
                .get(c)?
                .ok_or_else(|| Erro::Corrompido("pedaço faltando".into()))?;
            let v = g.value();
            let tam = u32_le(v)? as usize;
            texto.extend_from_slice(&self.codec.descomprimir(&v[4..], tam)?);
        }
        Ok(texto)
    }

    /// Reconstrói o texto de uma versão: anda pelas diferenças até um quadro-chave e volta
    /// aplicando; confere o blake3.
    fn reconstruir<V, P>(
        &self,
        versoes: &V,
        pedacos: &P,
        num: u32,
        id: u64,
        nota: &NotaId,
    ) -> Result<Vec<u8>>
    where
        V: ReadableTable<(u32, u64), &'static [u8]>,
        P: ReadableTable<[u8; 32], &'static [u8]>,
    {
        let alvo = registro(versoes, num, id)?
            .ok_or_else(|| Erro::NaoEncontrada(format!("versão {id} da nota {nota:?}")))?;
        let mut cadeia: Vec<Registro> = Vec::new();
        let mut atual = alvo.clone();
        let mut atual_id = id;
        let mut texto = loop {
            match &atual.corpo {
                Corpo::Cheia(chaves) => break self.texto_da_cheia(pedacos, chaves)?,
                Corpo::Delta { base, .. } => {
                    let base = *base;
                    if base <= atual_id || cadeia.len() > 1_000_000 {
                        return Err(Erro::Corrompido("cadeia de diferenças inválida".into()));
                    }
                    let prox = registro(versoes, num, base)?
                        .ok_or_else(|| Erro::Corrompido(format!("versão base {base} faltando")))?;
                    cadeia.push(atual);
                    atual = prox;
                    atual_id = base;
                }
            }
        };
        for r in cadeia.iter().rev() {
            let Corpo::Delta { dados, .. } = &r.corpo else {
                unreachable!("só diferenças entram na cadeia")
            };
            texto = self
                .codec
                .descomprimir_com_base(dados, &texto, r.tamanho as usize)?;
        }
        if hash(&texto) != alvo.hash {
            return Err(Erro::Integridade {
                nota: nota.0.clone(),
                versao: id,
            });
        }
        Ok(texto)
    }

    // ---------------------------------------------------------------- gravar

    /// Grava uma versão. Devolve `None` (e não grava nada) se o texto é idêntico ao da última
    /// versão da nota. `nota` é o id estável da nota, não o caminho do arquivo.
    ///
    /// Custo: corta o texto novo em pedaços, grava os que ainda não existem e comprime a versão
    /// anterior como diferença do texto novo. Uma transação, sem reconstruir nada.
    pub fn gravar(
        &mut self,
        nota: &NotaId,
        texto: &str,
        quando: SystemTime,
        origem: Origem,
    ) -> Result<Option<VersaoId>> {
        let bytes = texto.as_bytes();
        let h = hash(bytes);
        let w = self.txn_gravacao(false)?;
        let id_novo;
        {
            let mut notas = w.open_table(NOTAS)?;
            let mut versoes = w.open_table(VERSOES)?;
            let mut pedacos = w.open_table(PEDACOS)?;
            let mut refs = w.open_table(REFS)?;
            let num = match numero_da_nota(&notas, nota)? {
                Some(n) => n,
                None => {
                    let mut meta = w.open_table(META)?;
                    let n = meta
                        .get("proxima_nota")?
                        .map(|g| u32_le(g.value()))
                        .transpose()?
                        .unwrap_or(1);
                    meta.insert("proxima_nota", (n + 1).to_le_bytes().as_slice())?;
                    notas.insert(nota.como_str(), n)?;
                    n
                }
            };
            let anterior = ultima(&versoes, num)?;
            if let Some((_, r)) = &anterior
                && r.hash == h
            {
                return Ok(None);
            }
            id_novo = anterior.as_ref().map_or(1, |(id, _)| id + 1);

            let mut chaves = Vec::new();
            for f in cortar(bytes, &self.opc.pedacos) {
                let c = chave(&bytes[f.clone()]);
                self.guardar_pedaco(&mut pedacos, &mut refs, &c, &bytes[f])?;
                chaves.push(c);
            }

            let mut chave_anterior = 0;
            if let Some((id_ant, r)) = anterior {
                let dist = id_ant - r.chave_anterior;
                if dist >= self.opc.quadro_chave_a_cada.max(1) {
                    // Esta versão fica como quadro-chave (cópia cheia).
                    chave_anterior = id_ant;
                } else {
                    chave_anterior = r.chave_anterior;
                    let Corpo::Cheia(velhas) = &r.corpo else {
                        return Err(Erro::Corrompido("a versão mais nova não é cheia".into()));
                    };
                    let antigo = self.texto_da_cheia(&pedacos, velhas)?;
                    if hash(&antigo) != r.hash {
                        return Err(Erro::Integridade {
                            nota: nota.0.clone(),
                            versao: id_ant,
                        });
                    }
                    let dados = self.codec.comprimir_com_base(&antigo, bytes)?;
                    let nova = Registro {
                        corpo: Corpo::Delta {
                            base: id_novo,
                            dados,
                        },
                        ..r.clone()
                    };
                    versoes.insert((num, id_ant), nova.codificar(id_ant).as_slice())?;
                    for c in velhas {
                        self.soltar_pedaco(&mut pedacos, &mut refs, c)?;
                    }
                }
            }
            let novo = Registro {
                origem,
                quando_ms: ms_de(quando),
                tamanho: u32::try_from(bytes.len())
                    .map_err(|_| Erro::Argumento("nota maior que 4 GiB".into()))?,
                hash: h,
                chave_anterior,
                nome: None,
                corpo: Corpo::Cheia(chaves),
            };
            versoes.insert((num, id_novo), novo.codificar(id_novo).as_slice())?;
        }
        w.commit()?;
        Ok(Some(VersaoId(id_novo)))
    }

    // ----------------------------------------------------------------- ler

    /// As versões de uma nota, da mais antiga para a mais nova. Nota desconhecida: lista vazia.
    pub fn versoes(&self, nota: &NotaId) -> Result<Vec<VersaoInfo>> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(NOTAS)?;
        let Some(num) = numero_da_nota(&notas, nota)? else {
            return Ok(Vec::new());
        };
        let versoes = r.open_table(VERSOES)?;
        Ok(todas(&versoes, num)?
            .into_iter()
            .map(|(id, reg)| reg.info(id))
            .collect())
    }

    /// O texto de uma versão, conferido contra o blake3 gravado.
    pub fn ler(&self, nota: &NotaId, versao: VersaoId) -> Result<String> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(NOTAS)?;
        let num = numero_da_nota(&notas, nota)?
            .ok_or_else(|| Erro::NaoEncontrada(format!("nota {nota:?}")))?;
        let versoes = r.open_table(VERSOES)?;
        let pedacos = r.open_table(PEDACOS)?;
        texto_de(self.reconstruir(&versoes, &pedacos, num, versao.0, nota)?)
    }

    /// Todas as notas que têm histórico.
    pub fn notas(&self) -> Result<Vec<NotaId>> {
        Ok(self
            .todas_as_notas()?
            .into_iter()
            .map(|(n, _)| NotaId(n))
            .collect())
    }

    fn todas_as_notas(&self) -> Result<Vec<(String, u32)>> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(NOTAS)?;
        let mut v = Vec::new();
        for e in notas.iter()? {
            let (k, n) = e?;
            v.push((k.value().to_owned(), n.value()));
        }
        Ok(v)
    }

    // -------------------------------------------------------------- nomear

    /// Dá (ou tira, com `None` ou texto vazio) um nome a uma versão. Versões com nome nunca são podadas.
    pub fn nomear(&mut self, nota: &NotaId, versao: VersaoId, nome: Option<String>) -> Result<()> {
        let nome = nome.filter(|n| !n.is_empty());
        if nome.as_ref().is_some_and(|n| n.len() > 1024) {
            return Err(Erro::Argumento("nome com mais de 1024 bytes".into()));
        }
        let w = self.txn_gravacao(false)?;
        {
            let notas = w.open_table(NOTAS)?;
            let mut versoes = w.open_table(VERSOES)?;
            let num = numero_da_nota(&notas, nota)?
                .ok_or_else(|| Erro::NaoEncontrada(format!("nota {nota:?}")))?;
            let mut r = registro(&versoes, num, versao.0)?
                .ok_or_else(|| Erro::NaoEncontrada(format!("versão {} de {nota:?}", versao.0)))?;
            r.nome = nome;
            versoes.insert((num, versao.0), r.codificar(versao.0).as_slice())?;
        }
        w.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------ renomear

    /// Passa o histórico de `de` para o id `para` (a nota foi renomeada ou movida): as versões, os
    /// nomes e os números continuam os mesmos. Devolve `false` (e não faz nada) se `de` não tem
    /// histórico ou se `de == para`. Se `para` já tem histórico, devolve `Erro::Argumento` e nada
    /// muda: juntar dois históricos é decisão de quem chama.
    ///
    /// Custo: duas linhas da tabela `notas`; as versões e os pedaços não são tocados.
    pub fn renomear_nota(&mut self, de: &NotaId, para: &NotaId) -> Result<bool> {
        if de == para {
            return Ok(false);
        }
        let w = self.txn_gravacao(false)?;
        {
            let mut notas = w.open_table(NOTAS)?;
            let Some(num) = numero_da_nota(&notas, de)? else {
                return Ok(false);
            };
            if numero_da_nota(&notas, para)?.is_some() {
                return Err(Erro::Argumento(format!("a nota {para:?} já tem histórico")));
            }
            notas.remove(de.como_str())?;
            notas.insert(para.como_str(), num)?;
        }
        w.commit()?;
        Ok(true)
    }

    /// Ajusta de quantas em quantas versões há um quadro-chave nas **próximas** gravações. Vale
    /// trocar no meio do uso: cada registro guarda a distância até o seu quadro-chave, então bancos
    /// gravados com N diferentes continuam abrindo. Notas grandes pedem N menor (ver [`Opcoes`]).
    pub fn ajustar_quadro_chave(&mut self, a_cada: u64) {
        self.opc.quadro_chave_a_cada = a_cada.max(1);
    }

    // ------------------------------------------------------ reescrever cadeia

    /// Reescreve a cadeia inteira de uma nota numa passada, da mais nova para a mais antiga,
    /// guardando só dois textos na memória (o vizinho original, para decodificar, e o vizinho novo,
    /// para recodificar). Remove versões, troca textos e/ou redistribui os quadros-chave,
    /// reaproveitando os registros que não precisam mudar. Devolve quantos textos foram trocados.
    fn reescrever(
        &self,
        nota: &NotaId,
        versoes: &mut TVersoes,
        pedacos: &mut TPedacos,
        refs: &mut TRefs,
        num: u32,
        m: &Mudancas,
    ) -> Result<usize> {
        let itens = todas(versoes, num)?;
        let n = itens.len();
        let Some((id_mais_nova, _)) = itens.last() else {
            return Ok(0);
        };
        if m.remover.contains(id_mais_nova) {
            return Err(Erro::Argumento(
                "a versão mais nova da nota não pode ser removida".into(),
            ));
        }
        let max_cadeia = self.opc.quadro_chave_a_cada.max(1) as usize;

        // Plano: quem é quadro-chave (cheia). Da mais nova para a mais antiga.
        let mut cheia = vec![false; n];
        let mut desde_chave = 0usize;
        let mut primeira = true;
        for i in (0..n).rev() {
            if m.remover.contains(&itens[i].0) {
                continue;
            }
            if primeira {
                cheia[i] = true;
                primeira = false;
                continue;
            }
            desde_chave += 1;
            if (!m.refazer && itens[i].1.e_cheia()) || desde_chave >= max_cadeia {
                cheia[i] = true;
                desde_chave = 0;
            }
        }
        // Quadro-chave anterior de cada sobrevivente (da mais antiga para a mais nova).
        let mut chave_ant = vec![0u64; n];
        let mut ultimo = 0u64;
        for i in 0..n {
            if m.remover.contains(&itens[i].0) {
                continue;
            }
            chave_ant[i] = ultimo;
            if cheia[i] {
                ultimo = itens[i].0;
            }
        }

        let mut a_soltar: Vec<Chave> = Vec::new();
        let mut trocadas = 0usize;
        // Vizinho imediato mais novo (original, para decodificar a diferença que aponta para ele).
        let mut orig_prox: Vec<u8> = Vec::new();
        // Vizinho sobrevivente mais novo (já com a troca, para recodificar).
        let mut viz: Option<(u64, Vec<u8>, bool)> = None;

        for i in (0..n).rev() {
            let (id, reg) = &itens[i];
            let id = *id;
            let orig = match &reg.corpo {
                Corpo::Cheia(ch) => self.texto_da_cheia(pedacos, ch)?,
                Corpo::Delta { base, dados } => {
                    if i + 1 >= n || itens[i + 1].0 != *base {
                        return Err(Erro::Corrompido(
                            "a base da diferença não é a versão vizinha".into(),
                        ));
                    }
                    self.codec
                        .descomprimir_com_base(dados, &orig_prox, reg.tamanho as usize)?
                }
            };
            if hash(&orig) != reg.hash {
                return Err(Erro::Integridade {
                    nota: nota.0.clone(),
                    versao: id,
                });
            }

            if m.remover.contains(&id) {
                versoes.remove((num, id))?;
                if let Corpo::Cheia(ch) = &reg.corpo {
                    a_soltar.extend_from_slice(ch);
                }
                orig_prox = orig;
                continue;
            }

            let (novo, mudou) = match m.trocar.and_then(|f| f(&String::from_utf8_lossy(&orig))) {
                Some(t) if t.as_bytes() != orig.as_slice() => {
                    trocadas += 1;
                    (t.into_bytes(), true)
                }
                _ => (orig.clone(), false),
            };
            let (h_novo, tam_novo) = if mudou {
                (
                    hash(&novo),
                    u32::try_from(novo.len())
                        .map_err(|_| Erro::Argumento("nota maior que 4 GiB".into()))?,
                )
            } else {
                (reg.hash, reg.tamanho)
            };

            let reaproveita = !mudou
                && match (&reg.corpo, cheia[i], &viz) {
                    (Corpo::Cheia(_), true, _) => true,
                    (Corpo::Delta { base, .. }, false, Some((id_viz, _, mudou_viz))) => {
                        base == id_viz && !mudou_viz
                    }
                    _ => false,
                };
            let corpo = if reaproveita {
                reg.corpo.clone()
            } else {
                if let Corpo::Cheia(ch) = &reg.corpo {
                    a_soltar.extend_from_slice(ch);
                }
                if cheia[i] {
                    let mut chaves = Vec::new();
                    for f in cortar(&novo, &self.opc.pedacos) {
                        let c = chave(&novo[f.clone()]);
                        self.guardar_pedaco(pedacos, refs, &c, &novo[f])?;
                        chaves.push(c);
                    }
                    Corpo::Cheia(chaves)
                } else {
                    let (id_viz, t_viz, _) = viz
                        .as_ref()
                        .expect("só a mais nova sobrevivente não tem vizinha, e ela é cheia");
                    Corpo::Delta {
                        base: *id_viz,
                        dados: self.codec.comprimir_com_base(&novo, t_viz)?,
                    }
                }
            };
            let novo_reg = Registro {
                origem: reg.origem,
                quando_ms: reg.quando_ms,
                tamanho: tam_novo,
                hash: h_novo,
                chave_anterior: chave_ant[i],
                nome: reg.nome.clone(),
                corpo,
            };
            if reaproveita && novo_reg == *reg {
                // Nada mudou neste registro.
            } else {
                versoes.insert((num, id), novo_reg.codificar(id).as_slice())?;
            }
            viz = Some((id, novo, mudou));
            orig_prox = orig;
        }
        for c in &a_soltar {
            self.soltar_pedaco(pedacos, refs, c)?;
        }
        Ok(trocadas)
    }

    fn reescrever_nota(&mut self, nota: &NotaId, num: u32, m: &Mudancas) -> Result<usize> {
        let w = self.txn_gravacao(true)?;
        let n;
        {
            let mut versoes = w.open_table(VERSOES)?;
            let mut pedacos = w.open_table(PEDACOS)?;
            let mut refs = w.open_table(REFS)?;
            n = self.reescrever(nota, &mut versoes, &mut pedacos, &mut refs, num, m)?;
        }
        w.commit()?;
        Ok(n)
    }

    // --------------------------------------------------------------- podar

    /// Aplica a política de retenção a todas as notas (ver [`crate::retencao`]). Nunca remove a
    /// versão mais nova de uma nota nem as que têm nome, e recodifica as diferenças vizinhas para
    /// que toda versão que sobra continue abrindo igual.
    ///
    /// Custo por nota afetada: reconstrói a cadeia inteira (uma descompressão por versão) e
    /// recodifica só as vizinhas das removidas; ~20 us por versão. O arquivo não encolhe sozinho:
    /// use [`Historico::compactar`] para devolver o espaço ao sistema.
    pub fn podar(
        &mut self,
        agora: SystemTime,
        politica: &PoliticaRetencao,
    ) -> Result<RelatorioPoda> {
        let agora_ms = ms_de(agora);
        let mut rel = RelatorioPoda::default();
        for (nome, num) in self.todas_as_notas()? {
            let candidatas: Vec<Candidata> = {
                let r = self.db.begin_read()?;
                let versoes = r.open_table(VERSOES)?;
                todas(&versoes, num)?
                    .into_iter()
                    .map(|(id, reg)| Candidata {
                        id,
                        quando_ms: reg.quando_ms,
                        nomeada: reg.nome.is_some(),
                    })
                    .collect()
            };
            let remover = escolher_remocoes(&candidatas, agora_ms, politica);
            if remover.is_empty() {
                continue;
            }
            self.reescrever_nota(
                &NotaId(nome),
                num,
                &Mudancas {
                    remover: &remover,
                    trocar: None,
                    refazer: false,
                },
            )?;
            rel.versoes_removidas += remover.len();
            rel.notas_afetadas += 1;
        }
        Ok(rel)
    }

    // ------------------------------------------------------------ compactar

    /// Refaz os quadros-chave e as cadeias de todas as notas (um quadro-chave a cada N versões,
    /// contando da mais nova, sem os que a poda deixou sobrando) e devolve o espaço livre do
    /// arquivo ao sistema (`redb::Database::compact`).
    ///
    /// Custo: reconstrói e recodifica todas as versões de todas as notas, e reescreve o arquivo.
    /// Rode em segundo plano e raramente (ao fechar o cofre, depois de podar), nunca na linha da tela.
    pub fn compactar(&mut self) -> Result<RelatorioCompactacao> {
        let antes = self.tamanho_em_disco()?;
        let vazio = HashSet::new();
        for (nome, num) in self.todas_as_notas()? {
            self.reescrever_nota(
                &NotaId(nome),
                num,
                &Mudancas {
                    remover: &vazio,
                    trocar: None,
                    refazer: true,
                },
            )?;
        }
        self.db.compact()?;
        self.nao_persistido = false;
        Ok(RelatorioCompactacao {
            bytes_antes: antes,
            bytes_depois: self.tamanho_em_disco()?,
        })
    }

    // -------------------------------------------------------------- apagar

    /// Faz `trecho` sumir de **todas** as versões da nota (para uma senha colada por engano):
    /// cada ocorrência é removida do texto de cada versão que a contém; os pedaços e as diferenças
    /// que a continham são regravados e os pedaços antigos, apagados do banco. Devolve em quantas
    /// versões o trecho aparecia.
    ///
    /// Limites que quem chama precisa saber:
    /// - Só some o trecho **inteiro**. Versões gravadas enquanto a senha era digitada podem ter um
    ///   pedaço dela ("senh"); chame de novo com os pedaços que sobraram, ou apague a versão.
    /// - Tire também o trecho do `.md` real: a próxima gravação da nota o traria de volta.
    /// - Custo: reconstrói e recodifica a cadeia inteira da nota (~100 us por versão) e, no fim, o
    ///   arquivo é compactado (`redb` reaproveita páginas livres sem apagá-las: sem isso, bytes
    ///   comprimidos da senha poderiam ficar em páginas soltas do arquivo). Rode em segundo plano.
    /// - Só trata a nota pedida; se o mesmo texto foi colado em outra nota, repita nela.
    pub fn apagar_texto_do_historico(&mut self, nota: &NotaId, trecho: &str) -> Result<usize> {
        if trecho.is_empty() {
            return Err(Erro::Argumento("trecho vazio".into()));
        }
        let n = self.apagar_texto_sem_compactar(nota, trecho)?;
        if n > 0 {
            self.db.compact()?;
            self.nao_persistido = false;
        }
        Ok(n)
    }

    /// Como [`Historico::apagar_texto_do_historico`], mas em **todas** as notas (uma senha colada
    /// em mais de uma). O arquivo é compactado uma vez, no fim, e só se algo foi apagado. Devolve
    /// em quantas notas e em quantas versões o trecho aparecia. Valem os mesmos limites: só some o
    /// trecho inteiro, e é preciso tirá-lo também dos `.md`.
    ///
    /// Custo: reconstrói e recodifica a cadeia de cada nota que tem o trecho; as outras só são
    /// lidas. Rode em segundo plano.
    pub fn apagar_texto_de_todas(&mut self, trecho: &str) -> Result<ApagadoEmTodas> {
        if trecho.is_empty() {
            return Err(Erro::Argumento("trecho vazio".into()));
        }
        let mut total = ApagadoEmTodas::default();
        for (nome, _) in self.todas_as_notas()? {
            let n = self.apagar_texto_sem_compactar(&NotaId(nome), trecho)?;
            if n > 0 {
                total.notas += 1;
                total.versoes += n;
            }
        }
        if total.versoes > 0 {
            self.db.compact()?;
            self.nao_persistido = false;
        }
        Ok(total)
    }

    /// O miolo de [`Historico::apagar_texto_do_historico`], sem compactar o arquivo. `trecho` não vazio.
    fn apagar_texto_sem_compactar(&mut self, nota: &NotaId, trecho: &str) -> Result<usize> {
        let num = {
            let r = self.db.begin_read()?;
            let notas = r.open_table(NOTAS)?;
            match numero_da_nota(&notas, nota)? {
                Some(n) => n,
                None => return Ok(0),
            }
        };
        let vazio = HashSet::new();
        let trocar = |t: &str| t.contains(trecho).then(|| t.replace(trecho, ""));
        self.reescrever_nota(
            nota,
            num,
            &Mudancas {
                remover: &vazio,
                trocar: Some(&trocar),
                refazer: false,
            },
        )
    }

    // --------------------------------------------------------- estatísticas

    /// Tamanho do arquivo `.redb` no disco.
    pub fn tamanho_em_disco(&self) -> Result<u64> {
        Ok(std::fs::metadata(&self.caminho)?.len())
    }

    /// Quantas versões e quantos bytes uma nota usa.
    pub fn estatisticas_nota(&self, nota: &NotaId) -> Result<EstatisticasNota> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(NOTAS)?;
        let Some(num) = numero_da_nota(&notas, nota)? else {
            return Ok(EstatisticasNota::default());
        };
        let versoes = r.open_table(VERSOES)?;
        let pedacos = r.open_table(PEDACOS)?;
        let mut e = EstatisticasNota::default();
        for e_v in versoes.range((num, 0)..=(num, u64::MAX))? {
            let (k, v) = e_v?;
            let id = k.value().1;
            let bytes = v.value();
            let reg = Registro::decodificar(id, bytes)?;
            e.versoes += 1;
            e.bytes_texto += u64::from(reg.tamanho);
            e.bytes_guardados += bytes.len() as u64;
            if let Corpo::Cheia(ch) = &reg.corpo {
                for c in ch {
                    if let Some(g) = pedacos.get(c)? {
                        e.bytes_guardados += g.value().len() as u64;
                    }
                }
            }
        }
        Ok(e)
    }

    /// Números do banco inteiro.
    pub fn estatisticas(&self) -> Result<Estatisticas> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(NOTAS)?;
        let versoes = r.open_table(VERSOES)?;
        let pedacos = r.open_table(PEDACOS)?;
        let mut e = Estatisticas {
            notas: notas.len()? as usize,
            versoes: versoes.len()? as usize,
            pedacos: pedacos.len()? as usize,
            tamanho_em_disco: self.tamanho_em_disco()?,
            ..Default::default()
        };
        for x in pedacos.iter()? {
            let (_, v) = x?;
            e.bytes_pedacos += v.value().len() as u64;
        }
        for x in versoes.iter()? {
            let (_, v) = x?;
            e.bytes_registros += v.value().len() as u64;
        }
        Ok(e)
    }

    /// Para testes: o conteúdo descomprimido de todos os pedaços (para procurar texto nos bytes).
    #[doc(hidden)]
    pub fn pedacos_descomprimidos(&self) -> Result<Vec<Vec<u8>>> {
        let r = self.db.begin_read()?;
        let pedacos = r.open_table(PEDACOS)?;
        let mut v = Vec::new();
        for x in pedacos.iter()? {
            let (_, val) = x?;
            let val = val.value();
            v.push(self.codec.descomprimir(&val[4..], u32_le(val)? as usize)?);
        }
        Ok(v)
    }

    /// Para testes: confere que cada pedaço referenciado existe e que a contagem de referências
    /// bate com o uso real (nenhum pedaço órfão, nenhum faltando).
    #[doc(hidden)]
    pub fn conferir_referencias(&self) -> Result<()> {
        let r = self.db.begin_read()?;
        let versoes = r.open_table(VERSOES)?;
        let pedacos = r.open_table(PEDACOS)?;
        let refs = r.open_table(REFS)?;
        let mut uso: HashMap<Chave, u32> = HashMap::new();
        for x in versoes.iter()? {
            let (k, v) = x?;
            let id = k.value().1;
            if let Corpo::Cheia(ch) = Registro::decodificar(id, v.value())?.corpo {
                for c in ch {
                    *uso.entry(c).or_default() += 1;
                }
            }
        }
        for (c, n) in &uso {
            let guardado = refs.get(c)?.map(|g| g.value());
            if guardado != Some(*n) {
                return Err(Erro::Corrompido(format!(
                    "referências do pedaço: gravado {guardado:?}, real {n}"
                )));
            }
            if pedacos.get(c)?.is_none() {
                return Err(Erro::Corrompido("pedaço referenciado não existe".into()));
            }
        }
        if pedacos.len()? as usize != uso.len() || refs.len()? as usize != uso.len() {
            return Err(Erro::Corrompido("pedaço órfão no banco".into()));
        }
        Ok(())
    }
}

impl Drop for Historico {
    fn drop(&mut self) {
        if self.nao_persistido {
            let _ = self.persistir();
        }
    }
}

impl std::fmt::Debug for Historico {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Historico")
            .field("caminho", &self.caminho)
            .finish()
    }
}
