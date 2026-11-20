//! H3, "desfazer que fica": a árvore de desfazer de cada nota, guardada no cofre de versões.
//!
//! O Ctrl+Z de verdade continua no editor (memória, um grupo de digitação por vez). Aqui mora o que
//! sobrevive: cada **passo** é um texto da nota num momento de pausa, ligado ao passo anterior (o
//! "pai"). Desfazer e digitar outra coisa não perde o caminho antigo: ele vira um **galho**.
//!
//! # Como funciona
//!
//! - [`Historico::desfazer_registrar`] recebe o texto que a nota tem agora. Igual ao passo atual:
//!   nada. Igual a um passo que já existe (o usuário desfez ou refez): a árvore só anda o ponteiro
//!   `atual`. Texto novo: nasce um filho do passo atual.
//! - [`Historico::desfazer_abrir`] confere o blake3 do texto do arquivo com o dos passos: se o
//!   texto é o de algum passo, a árvore vale e devolve os textos dos passos anteriores (para o
//!   editor semear o Ctrl+Z). Se não é (o arquivo mudou fora do aplicativo, ou pelo sync), a árvore
//!   **não se aplica**: ela fica arquivada como histórico e uma nova começa. Nunca se aplica passo
//!   sobre um texto diferente.
//! - [`Historico::desfazer_galhos`] lista as pontas da árvore; [`Historico::desfazer_ir_para`]
//!   anda para uma delas.
//!
//! # Tabelas do `redb`
//!
//! | Tabela    | Chave                          | Valor                                              |
//! |-----------|--------------------------------|----------------------------------------------------|
//! | `arvores` | (número da nota, geração)      | cabeçalho: passo atual, próximo id, contagens, datas |
//! | `nos`     | (número da nota, geração, id)  | registro de um passo: pai, quando, blake3, resumo, dados |
//!
//! O número da nota é o mesmo das versões: renomear a nota leva a árvore junto. Cada passo é uma
//! **diferença zstd com o texto do pai como base**; a cada 32 passos de profundidade há uma cópia
//! cheia (comprimida solta), para reconstruir sem andar mais de 31 diferenças. Quando o limite
//! manda descartar os passos mais antigos, os filhos deles viram raízes (regravados como cópia
//! cheia): a árvore vira uma floresta.
//!
//! # Limite
//!
//! [`LimitesDesfazer`]: por nota, 5.000 passos ou 2 MiB (o que vier primeiro). Passando, descarta
//! os mais antigos até 90% do limite; o passo atual nunca sai.

use crate::erro::{Erro, Result};
use crate::historico::{Historico, numero_da_nota};
use crate::registro::{Impressao, NotaId, impressao_de_bytes, ms_de, tempo_de};
use redb::{ReadableDatabase, ReadableTable, Table, TableDefinition};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::SystemTime;

pub(crate) const ARVORES: TableDefinition<(u32, u32), &[u8]> = TableDefinition::new("arvores");
pub(crate) const NOS: TableDefinition<(u32, u32, u32), &[u8]> = TableDefinition::new("nos");

type TArvores<'t> = Table<'t, (u32, u32), &'static [u8]>;
type TNos<'t> = Table<'t, (u32, u32, u32), &'static [u8]>;

/// A cada quantos passos de profundidade há uma cópia cheia.
const CHEIA_A_CADA: u8 = 32;
/// Quantas árvores arquivadas (de antes de uma mudança de fora) ficam por nota.
const ARQUIVADAS_POR_NOTA: usize = 2;
/// Tamanho máximo do resumo de um passo, em caracteres.
const RESUMO_MAXIMO: usize = 60;
/// Até quanto a poda deixa o limite para não podar a cada passo.
const FOLGA_DA_PODA: f64 = 0.9;
/// Quantos galhos o diálogo recebe, no máximo (os mais recentes).
const GALHOS_MAXIMO: usize = 200;

/// Identificador de um passo da árvore (único na geração da árvore).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoId(pub u32);

/// Quanto a árvore de uma nota pode crescer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitesDesfazer {
    /// Passos por nota.
    pub max_passos: usize,
    /// Bytes guardados por nota (registros + dados comprimidos).
    pub max_bytes: usize,
}

impl Default for LimitesDesfazer {
    fn default() -> Self {
        Self {
            max_passos: 5_000,
            max_bytes: 2 * 1024 * 1024,
        }
    }
}

/// O que o editor precisa para semear o Ctrl+Z.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AberturaDesfazer {
    /// `false`: o texto do arquivo não é o de nenhum passo; a árvore antiga foi arquivada e esta é nova.
    pub aplicada: bool,
    /// O passo que corresponde ao texto de agora.
    pub atual: NoId,
    /// Os textos dos passos anteriores no caminho (do mais antigo ao mais novo, sem o atual).
    pub ancestrais: Vec<String>,
    /// Quantos passos a árvore tem.
    pub passos: usize,
    /// As impressões do texto de todos os passos: a linha de fundo reconhece com elas um
    /// Ctrl+Z ou Ctrl+Y (texto que já é um passo) sem abrir o banco.
    pub impressoes: Vec<Impressao>,
}

/// O que [`Historico::desfazer_registrar`] fez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registrado {
    /// Nada mudou, ou ainda não há árvore para esta nota.
    Nada,
    /// Nasceu um passo novo (filho do atual).
    Novo(NoId),
    /// O texto é o de um passo que já existia (desfazer ou refazer): só o ponteiro andou.
    Movido(NoId),
}

/// Uma ponta da árvore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Galho {
    pub no: NoId,
    pub quando: SystemTime,
    /// O que aquele passo mudou, em poucas palavras ("texto novo..." ou "apagou: ...").
    pub trecho: String,
    /// Quantos passos tem da raiz até aqui.
    pub passos: usize,
    /// É o passo em que a nota está agora.
    pub atual: bool,
    /// Está logo abaixo do passo atual (dá para chegar com "refazer").
    pub refazer: bool,
}

/// Uma árvore de antes de uma mudança de fora do aplicativo: só histórico, nada se aplica a ela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArvoreArquivada {
    pub geracao: u32,
    pub arquivada_em: SystemTime,
    pub passos: usize,
}

/// Os galhos de uma nota.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Galhos {
    /// Do mais recente para o mais antigo.
    pub galhos: Vec<Galho>,
    pub arquivadas: Vec<ArvoreArquivada>,
    /// Quantos passos a árvore ativa tem.
    pub passos: usize,
}

// ------------------------------------------------------------------------------- formato

/// Cabeçalho de uma árvore (uma linha de `arvores`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cabecalho {
    atual: u32,
    proximo: u32,
    passos: u32,
    bytes: u64,
    criada_ms: u64,
    /// 0 = ativa; senão, quando foi arquivada.
    arquivada_ms: u64,
}

const CABECALHO_BYTES: usize = 1 + 4 + 4 + 4 + 8 + 8 + 8;

impl Cabecalho {
    fn codificar(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(CABECALHO_BYTES);
        o.push(1);
        o.extend_from_slice(&self.atual.to_le_bytes());
        o.extend_from_slice(&self.proximo.to_le_bytes());
        o.extend_from_slice(&self.passos.to_le_bytes());
        o.extend_from_slice(&self.bytes.to_le_bytes());
        o.extend_from_slice(&self.criada_ms.to_le_bytes());
        o.extend_from_slice(&self.arquivada_ms.to_le_bytes());
        o
    }

    fn decodificar(b: &[u8]) -> Result<Self> {
        if b.len() != CABECALHO_BYTES || b[0] != 1 {
            return Err(Erro::Corrompido("cabeçalho da árvore de desfazer".into()));
        }
        let u32_em = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().expect("4 bytes"));
        let u64_em = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().expect("8 bytes"));
        Ok(Self {
            atual: u32_em(1),
            proximo: u32_em(5),
            passos: u32_em(9),
            bytes: u64_em(13),
            criada_ms: u64_em(21),
            arquivada_ms: u64_em(29),
        })
    }
}

/// Um passo da árvore.
#[derive(Debug, Clone, PartialEq, Eq)]
struct No {
    /// 0 = raiz.
    pai: u32,
    quando_ms: u64,
    tamanho: u32,
    hash: Impressao,
    /// Cópia cheia (zstd solto) em vez de diferença contra o pai.
    cheia: bool,
    /// Quantas diferenças desde a última cópia cheia (0 na cheia).
    dist: u8,
    resumo: String,
    dados: Vec<u8>,
}

const FIXO_DO_NO: usize = 1 + 4 + 1 + 8 + 4 + 16 + 2;

impl No {
    fn codificar(&self) -> Vec<u8> {
        let resumo = self.resumo.as_bytes();
        let mut o = Vec::with_capacity(FIXO_DO_NO + resumo.len() + self.dados.len());
        o.push(u8::from(self.cheia));
        o.extend_from_slice(&self.pai.to_le_bytes());
        o.push(self.dist);
        o.extend_from_slice(&self.quando_ms.to_le_bytes());
        o.extend_from_slice(&self.tamanho.to_le_bytes());
        o.extend_from_slice(&self.hash);
        o.extend_from_slice(&(resumo.len() as u16).to_le_bytes());
        o.extend_from_slice(resumo);
        o.extend_from_slice(&self.dados);
        o
    }

    fn decodificar(b: &[u8]) -> Result<Self> {
        let trunc = || Erro::Corrompido("passo da árvore de desfazer truncado".into());
        if b.len() < FIXO_DO_NO {
            return Err(trunc());
        }
        let tamanho_resumo = u16::from_le_bytes([b[FIXO_DO_NO - 2], b[FIXO_DO_NO - 1]]) as usize;
        let fim_resumo = FIXO_DO_NO + tamanho_resumo;
        let resumo = b.get(FIXO_DO_NO..fim_resumo).ok_or_else(trunc)?;
        Ok(Self {
            cheia: b[0] & 1 == 1,
            pai: u32::from_le_bytes(b[1..5].try_into().expect("4 bytes")),
            dist: b[5],
            quando_ms: u64::from_le_bytes(b[6..14].try_into().expect("8 bytes")),
            tamanho: u32::from_le_bytes(b[14..18].try_into().expect("4 bytes")),
            hash: b[18..34].try_into().expect("16 bytes"),
            resumo: String::from_utf8(resumo.to_vec())
                .map_err(|_| Erro::Corrompido("resumo do passo não é UTF-8".into()))?,
            dados: b[fim_resumo..].to_vec(),
        })
    }

    /// Bytes que este passo ocupa no banco.
    fn bytes(&self) -> u64 {
        (FIXO_DO_NO + self.resumo.len() + self.dados.len()) as u64
    }
}

/// Uma árvore inteira na memória (cabeçalho e passos). O limite de tamanho a mantém pequena.
struct Arvore {
    geracao: u32,
    cab: Cabecalho,
    nos: BTreeMap<u32, No>,
}

/// O resumo do que um passo mudou: o que entrou (se entrou algo) ou "apagou: ...".
pub fn resumo_da_mudanca(antes: &str, depois: &str) -> String {
    let a: Vec<char> = antes.chars().collect();
    let d: Vec<char> = depois.chars().collect();
    let mut prefixo = 0;
    while prefixo < a.len() && prefixo < d.len() && a[prefixo] == d[prefixo] {
        prefixo += 1;
    }
    let mut sufixo = 0;
    while sufixo < a.len() - prefixo
        && sufixo < d.len() - prefixo
        && a[a.len() - 1 - sufixo] == d[d.len() - 1 - sufixo]
    {
        sufixo += 1;
    }
    let tirou: String = a[prefixo..a.len() - sufixo].iter().collect();
    let pos: String = d[prefixo..d.len() - sufixo].iter().collect();
    let curto = |s: &str| -> String {
        let junto = s.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut saida: String = junto.chars().take(RESUMO_MAXIMO).collect();
        if junto.chars().count() > RESUMO_MAXIMO {
            saida.push('…');
        }
        saida
    };
    if !pos.trim().is_empty() {
        curto(&pos)
    } else if !tirou.trim().is_empty() {
        format!("apagou: {}", curto(&tirou))
    } else {
        "ajuste de espaços".to_owned()
    }
}

impl Arvore {
    /// Os ids do caminho da raiz até `id`, só os `quantos + 1` últimos que bastam para reconstruir
    /// (sobe até uma cópia cheia depois de ter o suficiente).
    fn cadeia(&self, id: u32, quantos: usize) -> Result<Vec<u32>> {
        let mut ids = vec![id];
        let mut atual = id;
        loop {
            let n = self
                .nos
                .get(&atual)
                .ok_or_else(|| Erro::Corrompido(format!("passo {atual} faltando")))?;
            if n.cheia && ids.len() > quantos {
                break;
            }
            if n.pai == 0 {
                if !n.cheia {
                    return Err(Erro::Corrompido("raiz que não é cópia cheia".into()));
                }
                break;
            }
            if n.pai >= atual || ids.len() > 1_000_000 {
                return Err(Erro::Corrompido("pai inválido na árvore".into()));
            }
            atual = n.pai;
            ids.push(atual);
        }
        ids.reverse();
        Ok(ids)
    }

    /// Os textos dos últimos `quantos + 1` passos do caminho até `id` (o último é o de `id`),
    /// conferidos contra o blake3 gravado.
    fn caminho(&self, h: &Historico, id: u32, quantos: usize) -> Result<Vec<(u32, Vec<u8>)>> {
        let ids = self.cadeia(id, quantos)?;
        let mut saida: Vec<(u32, Vec<u8>)> = Vec::with_capacity(ids.len());
        for &i in &ids {
            let n = &self.nos[&i];
            let texto = if n.cheia {
                h.codec.descomprimir(&n.dados, n.tamanho as usize)?
            } else {
                let base = &saida
                    .last()
                    .ok_or_else(|| Erro::Corrompido("diferença sem base".into()))?
                    .1;
                h.codec
                    .descomprimir_com_base(&n.dados, base, n.tamanho as usize)?
            };
            if impressao_de_bytes(&texto) != n.hash {
                return Err(Erro::Integridade {
                    nota: "árvore de desfazer".into(),
                    versao: u64::from(i),
                });
            }
            saida.push((i, texto));
        }
        let corte = saida.len().saturating_sub(quantos + 1);
        Ok(saida.split_off(corte))
    }

    fn texto(&self, h: &Historico, id: u32) -> Result<Vec<u8>> {
        Ok(self
            .caminho(h, id, 0)?
            .pop()
            .map(|(_, t)| t)
            .unwrap_or_default())
    }

    /// Monta o passo de `texto` como filho de `pai` (com o texto dele), ou raiz.
    fn novo_no(
        &self,
        h: &Historico,
        pai: Option<(u32, &[u8])>,
        texto: &[u8],
        quando_ms: u64,
    ) -> Result<No> {
        let tamanho = u32::try_from(texto.len())
            .map_err(|_| Erro::Argumento("nota maior que 4 GiB".into()))?;
        let (cheia, dist) = match pai {
            None => (true, 0),
            Some((id_pai, _)) => {
                let dist = self.nos.get(&id_pai).map_or(CHEIA_A_CADA, |p| p.dist) + 1;
                if dist >= CHEIA_A_CADA {
                    (true, 0)
                } else {
                    (false, dist)
                }
            }
        };
        let (dados, resumo) = match pai {
            None => (h.codec.comprimir(texto)?, "texto inicial".to_owned()),
            Some((_, base)) => {
                let resumo = resumo_da_mudanca(
                    &String::from_utf8_lossy(base),
                    &String::from_utf8_lossy(texto),
                );
                let dados = if cheia {
                    h.codec.comprimir(texto)?
                } else {
                    h.codec.comprimir_com_base(texto, base)?
                };
                (dados, resumo)
            }
        };
        Ok(No {
            pai: pai.map_or(0, |(id, _)| id),
            quando_ms,
            tamanho,
            hash: impressao_de_bytes(texto),
            cheia,
            dist,
            resumo,
            dados,
        })
    }

    fn recontar(&mut self) {
        self.cab.passos = self.nos.len() as u32;
        self.cab.bytes = self.nos.values().map(No::bytes).sum();
    }
}

// ------------------------------------------------------------------------------- tabelas

fn carregar<A, N>(arvores: &A, nos: &N, num: u32, geracao: u32) -> Result<Option<Arvore>>
where
    A: ReadableTable<(u32, u32), &'static [u8]>,
    N: ReadableTable<(u32, u32, u32), &'static [u8]>,
{
    let Some(g) = arvores.get((num, geracao))? else {
        return Ok(None);
    };
    let cab = Cabecalho::decodificar(g.value())?;
    drop(g);
    let mut mapa = BTreeMap::new();
    for e in nos.range((num, geracao, 0)..=(num, geracao, u32::MAX))? {
        let (k, v) = e?;
        mapa.insert(k.value().2, No::decodificar(v.value())?);
    }
    Ok(Some(Arvore {
        geracao,
        cab,
        nos: mapa,
    }))
}

/// Todas as gerações da nota, da mais antiga para a mais nova, com o cabeçalho.
fn geracoes<A: ReadableTable<(u32, u32), &'static [u8]>>(
    arvores: &A,
    num: u32,
) -> Result<Vec<(u32, Cabecalho)>> {
    let mut v = Vec::new();
    for e in arvores.range((num, 0)..=(num, u32::MAX))? {
        let (k, val) = e?;
        v.push((k.value().1, Cabecalho::decodificar(val.value())?));
    }
    Ok(v)
}

fn ativa<A, N>(arvores: &A, nos: &N, num: u32) -> Result<Option<Arvore>>
where
    A: ReadableTable<(u32, u32), &'static [u8]>,
    N: ReadableTable<(u32, u32, u32), &'static [u8]>,
{
    match geracoes(arvores, num)?
        .into_iter()
        .rev()
        .find(|(_, c)| c.arquivada_ms == 0)
    {
        Some((g, _)) => carregar(arvores, nos, num, g),
        None => Ok(None),
    }
}

fn gravar_cabecalho(arvores: &mut TArvores, num: u32, arv: &Arvore) -> Result<()> {
    arvores.insert((num, arv.geracao), arv.cab.codificar().as_slice())?;
    Ok(())
}

fn gravar_no(nos: &mut TNos, num: u32, geracao: u32, id: u32, no: &No) -> Result<()> {
    nos.insert((num, geracao, id), no.codificar().as_slice())?;
    Ok(())
}

fn apagar_geracao(arvores: &mut TArvores, nos: &mut TNos, num: u32, geracao: u32) -> Result<()> {
    let ids: Vec<u32> = nos
        .range((num, geracao, 0)..=(num, geracao, u32::MAX))?
        .map(|e| e.map(|(k, _)| k.value().2))
        .collect::<std::result::Result<_, _>>()?;
    for id in ids {
        nos.remove((num, geracao, id))?;
    }
    arvores.remove((num, geracao))?;
    Ok(())
}

// ------------------------------------------------------------------------------- API

impl Historico {
    /// Muda o limite da árvore de cada nota (vale a partir do próximo passo registrado).
    pub fn ajustar_limites_desfazer(&mut self, limites: LimitesDesfazer) {
        self.limites_desfazer = LimitesDesfazer {
            max_passos: limites.max_passos.max(10),
            max_bytes: limites.max_bytes.max(16 * 1024),
        };
    }

    /// A nota apareceu com `texto` (o do arquivo). Devolve o que o editor precisa para semear o
    /// Ctrl+Z, com até `max_ancestrais` passos anteriores. Se não há árvore, ou se o texto não é o
    /// de nenhum passo (o arquivo mudou fora do aplicativo), a árvore antiga é arquivada e uma nova
    /// começa com `texto` como raiz (`aplicada: false`).
    pub fn desfazer_abrir(
        &mut self,
        nota: &NotaId,
        texto: &str,
        quando: SystemTime,
        max_ancestrais: usize,
    ) -> Result<AberturaDesfazer> {
        let h = impressao_de_bytes(texto.as_bytes());
        let w = self.txn_gravacao(false)?;
        let resultado;
        {
            let num = Historico::garantir_numero(&w, nota)?;
            let mut arvores = w.open_table(ARVORES)?;
            let mut nos = w.open_table(NOS)?;
            let existente = ativa(&arvores, &nos, num)?;
            let mut aplicada = None;
            if let Some(mut arv) = existente {
                let achado = if arv.nos.get(&arv.cab.atual).is_some_and(|n| n.hash == h) {
                    Some(arv.cab.atual)
                } else {
                    arv.nos
                        .iter()
                        .rev()
                        .find(|(_, n)| n.hash == h)
                        .map(|(id, _)| *id)
                };
                if let Some(no) = achado
                    && let Ok(caminho) = arv.caminho(self, no, max_ancestrais)
                {
                    if no != arv.cab.atual {
                        arv.cab.atual = no;
                        gravar_cabecalho(&mut arvores, num, &arv)?;
                    }
                    let mut textos: Vec<String> = caminho
                        .into_iter()
                        .map(|(_, t)| String::from_utf8_lossy(&t).into_owned())
                        .collect();
                    textos.pop();
                    aplicada = Some(AberturaDesfazer {
                        aplicada: true,
                        atual: NoId(no),
                        ancestrais: textos,
                        passos: arv.cab.passos as usize,
                        impressoes: arv.nos.values().map(|n| n.hash).collect(),
                    });
                } else {
                    // O texto de fora vale: a árvore antiga fica só como histórico.
                    arv.cab.arquivada_ms = ms_de(quando).max(1);
                    gravar_cabecalho(&mut arvores, num, &arv)?;
                }
            }
            resultado = match aplicada {
                Some(a) => a,
                None => {
                    self.criar_arvore(&mut arvores, &mut nos, num, texto.as_bytes(), quando)?;
                    AberturaDesfazer {
                        aplicada: false,
                        atual: NoId(1),
                        ancestrais: Vec::new(),
                        passos: 1,
                        impressoes: vec![h],
                    }
                }
            };
        }
        w.commit()?;
        Ok(resultado)
    }

    /// Nova geração com `texto` como raiz; descarta as arquivadas que passarem do limite.
    fn criar_arvore(
        &self,
        arvores: &mut TArvores,
        nos: &mut TNos,
        num: u32,
        texto: &[u8],
        quando: SystemTime,
    ) -> Result<()> {
        let todas = geracoes(arvores, num)?;
        let geracao = todas.last().map_or(1, |(g, _)| g + 1);
        let mut arv = Arvore {
            geracao,
            cab: Cabecalho {
                atual: 1,
                proximo: 2,
                passos: 1,
                bytes: 0,
                criada_ms: ms_de(quando),
                arquivada_ms: 0,
            },
            nos: BTreeMap::new(),
        };
        let raiz = arv.novo_no(self, None, texto, ms_de(quando))?;
        gravar_no(nos, num, geracao, 1, &raiz)?;
        arv.nos.insert(1, raiz);
        arv.recontar();
        gravar_cabecalho(arvores, num, &arv)?;
        let arquivadas: Vec<u32> = todas
            .iter()
            .filter(|(_, c)| c.arquivada_ms != 0)
            .map(|(g, _)| *g)
            .collect();
        if arquivadas.len() > ARQUIVADAS_POR_NOTA {
            for g in &arquivadas[..arquivadas.len() - ARQUIVADAS_POR_NOTA] {
                apagar_geracao(arvores, nos, num, *g)?;
            }
        }
        Ok(())
    }

    /// O texto da nota agora é `texto` (a linha de fundo chama depois de uma pausa na digitação,
    /// ao salvar, ao trocar de nota e ao fechar). Ver [`Registrado`]. Sem árvore para a nota,
    /// devolve `Registrado::Nada` (chame antes [`Historico::desfazer_abrir`]).
    pub fn desfazer_registrar(
        &mut self,
        nota: &NotaId,
        texto: &str,
        quando: SystemTime,
    ) -> Result<Registrado> {
        let h = impressao_de_bytes(texto.as_bytes());
        let w = self.txn_gravacao(false)?;
        let resultado;
        {
            let Some(num) = ({
                let notas = w.open_table(crate::historico::NOTAS)?;
                numero_da_nota(&notas, nota)?
            }) else {
                return Ok(Registrado::Nada);
            };
            let mut arvores = w.open_table(ARVORES)?;
            let mut nos = w.open_table(NOS)?;
            let Some(mut arv) = ativa(&arvores, &nos, num)? else {
                return Ok(Registrado::Nada);
            };
            if arv.nos.get(&arv.cab.atual).is_some_and(|n| n.hash == h) {
                return Ok(Registrado::Nada);
            }
            if let Some(id) = arv
                .nos
                .iter()
                .rev()
                .find(|(_, n)| n.hash == h)
                .map(|(id, _)| *id)
            {
                arv.cab.atual = id;
                gravar_cabecalho(&mut arvores, num, &arv)?;
                resultado = Registrado::Movido(NoId(id));
            } else {
                let pai = arv.cab.atual;
                let texto_pai = arv.texto(self, pai)?;
                let no = arv.novo_no(
                    self,
                    Some((pai, &texto_pai)),
                    texto.as_bytes(),
                    ms_de(quando),
                )?;
                let id = arv.cab.proximo;
                gravar_no(&mut nos, num, arv.geracao, id, &no)?;
                arv.cab.proximo = id + 1;
                arv.cab.atual = id;
                arv.cab.passos += 1;
                arv.cab.bytes += no.bytes();
                arv.nos.insert(id, no);
                let lim = self.limites_desfazer;
                if arv.cab.passos as usize > lim.max_passos
                    || arv.cab.bytes as usize > lim.max_bytes
                {
                    self.podar_arvore(&mut arv, &mut nos, num, lim)?;
                }
                gravar_cabecalho(&mut arvores, num, &arv)?;
                resultado = Registrado::Novo(NoId(id));
            }
        }
        w.commit()?;
        Ok(resultado)
    }

    /// Descarta os passos mais antigos (menores ids) até a árvore caber em 90% do limite. O passo
    /// atual fica sempre. Quem era filho de um passo descartado vira raiz (cópia cheia).
    fn podar_arvore(
        &self,
        arv: &mut Arvore,
        nos: &mut TNos,
        num: u32,
        lim: LimitesDesfazer,
    ) -> Result<()> {
        let alvo_passos = ((lim.max_passos as f64) * FOLGA_DA_PODA) as usize;
        let alvo_bytes = ((lim.max_bytes as f64) * FOLGA_DA_PODA) as u64;
        let mut passos = arv.nos.len();
        let mut bytes: u64 = arv.nos.values().map(No::bytes).sum();
        let mut remover: HashSet<u32> = HashSet::new();
        for (&id, no) in &arv.nos {
            if passos <= alvo_passos && bytes <= alvo_bytes {
                break;
            }
            if id == arv.cab.atual {
                continue;
            }
            remover.insert(id);
            passos -= 1;
            bytes -= no.bytes();
        }
        if remover.is_empty() {
            return Ok(());
        }
        // Os filhos dos descartados viram raízes: texto cheio, lido enquanto a cadeia ainda existe.
        let orfaos: Vec<u32> = arv
            .nos
            .iter()
            .filter(|(id, n)| !remover.contains(id) && remover.contains(&n.pai))
            .map(|(id, _)| *id)
            .collect();
        let mut novos = Vec::with_capacity(orfaos.len());
        for id in orfaos {
            let texto = arv.texto(self, id)?;
            let antigo = &arv.nos[&id];
            novos.push((
                id,
                No {
                    pai: 0,
                    quando_ms: antigo.quando_ms,
                    tamanho: antigo.tamanho,
                    hash: antigo.hash,
                    cheia: true,
                    dist: 0,
                    resumo: antigo.resumo.clone(),
                    dados: self.codec.comprimir(&texto)?,
                },
            ));
        }
        for id in &remover {
            nos.remove((num, arv.geracao, *id))?;
            arv.nos.remove(id);
        }
        for (id, no) in novos {
            gravar_no(nos, num, arv.geracao, id, &no)?;
            arv.nos.insert(id, no);
        }
        arv.recontar();
        Ok(())
    }

    /// As pontas da árvore da nota (e as árvores arquivadas).
    pub fn desfazer_galhos(&self, nota: &NotaId) -> Result<Galhos> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(crate::historico::NOTAS)?;
        let Some(num) = numero_da_nota(&notas, nota)? else {
            return Ok(Galhos::default());
        };
        let arvores = r.open_table(ARVORES)?;
        let nos = r.open_table(NOS)?;
        let mut saida = Galhos::default();
        for (g, c) in geracoes(&arvores, num)? {
            if c.arquivada_ms != 0 {
                saida.arquivadas.push(ArvoreArquivada {
                    geracao: g,
                    arquivada_em: tempo_de(c.arquivada_ms),
                    passos: c.passos as usize,
                });
            }
        }
        let Some(arv) = ativa(&arvores, &nos, num)? else {
            return Ok(saida);
        };
        saida.passos = arv.nos.len();
        let com_filhos: HashSet<u32> = arv.nos.values().map(|n| n.pai).collect();
        // Profundidade: o pai sempre tem id menor que o filho.
        let mut fundo: HashMap<u32, usize> = HashMap::new();
        for (&id, n) in &arv.nos {
            let d = fundo.get(&n.pai).map_or(1, |p| p + 1);
            fundo.insert(id, d);
        }
        let abaixo_do_atual = |mut id: u32| -> bool {
            while let Some(n) = arv.nos.get(&id) {
                if n.pai == arv.cab.atual {
                    return true;
                }
                if n.pai == 0 {
                    return false;
                }
                id = n.pai;
            }
            false
        };
        for (&id, n) in &arv.nos {
            let e_atual = id == arv.cab.atual;
            if com_filhos.contains(&id) && !e_atual {
                continue;
            }
            saida.galhos.push(Galho {
                no: NoId(id),
                quando: tempo_de(n.quando_ms),
                trecho: n.resumo.clone(),
                passos: fundo[&id],
                atual: e_atual,
                refazer: !e_atual && abaixo_do_atual(id),
            });
        }
        saida.galhos.sort_by(|a, b| {
            b.atual
                .cmp(&a.atual)
                .then(b.quando.cmp(&a.quando))
                .then(b.no.cmp(&a.no))
        });
        saida.galhos.truncate(GALHOS_MAXIMO);
        Ok(saida)
    }

    /// Anda a nota para um passo da árvore: devolve o texto dele e os `max_ancestrais` anteriores
    /// para o editor refazer a semente do Ctrl+Z. O passo vira o atual.
    pub fn desfazer_ir_para(
        &mut self,
        nota: &NotaId,
        no: NoId,
        max_ancestrais: usize,
    ) -> Result<(String, AberturaDesfazer)> {
        let w = self.txn_gravacao(false)?;
        let resultado;
        {
            let num = {
                let notas = w.open_table(crate::historico::NOTAS)?;
                numero_da_nota(&notas, nota)?
                    .ok_or_else(|| Erro::NaoEncontrada(format!("nota {nota:?}")))?
            };
            let mut arvores = w.open_table(ARVORES)?;
            let nos = w.open_table(NOS)?;
            let mut arv = ativa(&arvores, &nos, num)?
                .ok_or_else(|| Erro::NaoEncontrada(format!("árvore de desfazer de {nota:?}")))?;
            if !arv.nos.contains_key(&no.0) {
                return Err(Erro::NaoEncontrada(format!("passo {} da árvore", no.0)));
            }
            let mut caminho = arv.caminho(self, no.0, max_ancestrais)?;
            let (_, texto) = caminho.pop().expect("o caminho tem o próprio passo");
            let texto = String::from_utf8(texto)
                .map_err(|_| Erro::Corrompido("passo não é UTF-8".into()))?;
            arv.cab.atual = no.0;
            gravar_cabecalho(&mut arvores, num, &arv)?;
            resultado = (
                texto,
                AberturaDesfazer {
                    aplicada: true,
                    atual: no,
                    ancestrais: caminho
                        .into_iter()
                        .map(|(_, t)| String::from_utf8_lossy(&t).into_owned())
                        .collect(),
                    passos: arv.nos.len(),
                    impressoes: arv.nos.values().map(|n| n.hash).collect(),
                },
            );
        }
        w.commit()?;
        Ok(resultado)
    }

    /// O texto de um passo da árvore ativa.
    pub fn desfazer_texto(&self, nota: &NotaId, no: NoId) -> Result<String> {
        let r = self.db.begin_read()?;
        let notas = r.open_table(crate::historico::NOTAS)?;
        let num = numero_da_nota(&notas, nota)?
            .ok_or_else(|| Erro::NaoEncontrada(format!("nota {nota:?}")))?;
        let arvores = r.open_table(ARVORES)?;
        let nos = r.open_table(NOS)?;
        let arv = ativa(&arvores, &nos, num)?
            .ok_or_else(|| Erro::NaoEncontrada(format!("árvore de desfazer de {nota:?}")))?;
        if !arv.nos.contains_key(&no.0) {
            return Err(Erro::NaoEncontrada(format!("passo {} da árvore", no.0)));
        }
        String::from_utf8(arv.texto(self, no.0)?)
            .map_err(|_| Erro::Corrompido("passo não é UTF-8".into()))
    }

    /// Tira `trecho` do texto de todos os passos de todas as árvores da nota (ativa e arquivadas).
    /// Devolve quantos passos mudaram. Chamado por `apagar_texto_do_historico`.
    pub(crate) fn desfazer_apagar(&mut self, num: u32, trecho: &str) -> Result<usize> {
        let geracoes_da_nota = {
            let r = self.db.begin_read()?;
            let arvores = r.open_table(ARVORES)?;
            geracoes(&arvores, num)?
        };
        if geracoes_da_nota.is_empty() {
            return Ok(0);
        }
        let mut total = 0;
        for (g, _) in geracoes_da_nota {
            let arv = {
                let r = self.db.begin_read()?;
                let arvores = r.open_table(ARVORES)?;
                let nos = r.open_table(NOS)?;
                carregar(&arvores, &nos, num, g)?
            };
            let Some(arv) = arv else { continue };
            let Some((nova, mudou)) = self.reescrever_arvore(&arv, trecho)? else {
                continue;
            };
            let w = self.txn_gravacao(true)?;
            {
                let mut arvores = w.open_table(ARVORES)?;
                let mut nos = w.open_table(NOS)?;
                for (id, no) in &nova.nos {
                    gravar_no(&mut nos, num, g, *id, no)?;
                }
                gravar_cabecalho(&mut arvores, num, &nova)?;
            }
            w.commit()?;
            total += mudou;
        }
        Ok(total)
    }

    /// Refaz uma árvore sem `trecho`. `None` se nenhum passo o contém.
    fn reescrever_arvore(&self, arv: &Arvore, trecho: &str) -> Result<Option<(Arvore, usize)>> {
        let t = trecho.as_bytes();
        let contem = |texto: &[u8]| texto.windows(t.len()).any(|j| j == t);
        let filhos = |arv: &Arvore| {
            let mut c: HashMap<u32, usize> = HashMap::new();
            for n in arv.nos.values() {
                *c.entry(n.pai).or_default() += 1;
            }
            c
        };
        // Primeira passada: algum passo tem o trecho? Só decodifica, nada é recodificado.
        let mut restam = filhos(arv);
        let mut originais: HashMap<u32, Vec<u8>> = HashMap::new();
        let mut achou = false;
        for (&id, n) in &arv.nos {
            let texto = self.decodificar_no(n, originais.get(&n.pai).map(Vec::as_slice))?;
            achou |= contem(&texto);
            liberar(&mut restam, &mut originais, n.pai);
            if restam.get(&id).copied().unwrap_or(0) > 0 {
                originais.insert(id, texto);
            }
        }
        if !achou {
            return Ok(None);
        }
        // Segunda: refaz cada passo sobre o texto já limpo do pai.
        let mut nova = Arvore {
            geracao: arv.geracao,
            cab: arv.cab,
            nos: BTreeMap::new(),
        };
        let mut restam = filhos(arv);
        let mut originais: HashMap<u32, Vec<u8>> = HashMap::new();
        let mut limpos: HashMap<u32, Vec<u8>> = HashMap::new();
        let mut mudou = 0;
        for (&id, n) in &arv.nos {
            let original = self.decodificar_no(n, originais.get(&n.pai).map(Vec::as_slice))?;
            let limpo = if contem(&original) {
                mudou += 1;
                String::from_utf8_lossy(&original)
                    .replace(trecho, "")
                    .into_bytes()
            } else {
                original.clone()
            };
            let pai = limpos.get(&n.pai).map(Vec::as_slice);
            let tamanho = u32::try_from(limpo.len())
                .map_err(|_| Erro::Argumento("nota maior que 4 GiB".into()))?;
            let dados = match (n.cheia, pai) {
                (false, Some(base)) => self.codec.comprimir_com_base(&limpo, base)?,
                _ => self.codec.comprimir(&limpo)?,
            };
            let resumo = match pai {
                Some(base) => resumo_da_mudanca(
                    &String::from_utf8_lossy(base),
                    &String::from_utf8_lossy(&limpo),
                ),
                None => n.resumo.clone(),
            };
            nova.nos.insert(
                id,
                No {
                    pai: n.pai,
                    quando_ms: n.quando_ms,
                    tamanho,
                    hash: impressao_de_bytes(&limpo),
                    cheia: n.cheia || pai.is_none(),
                    dist: n.dist,
                    resumo,
                    dados,
                },
            );
            liberar(&mut restam, &mut originais, n.pai);
            liberar_limpo(&restam, &mut limpos, n.pai);
            if restam.get(&id).copied().unwrap_or(0) > 0 {
                originais.insert(id, original);
                limpos.insert(id, limpo);
            }
        }
        nova.recontar();
        Ok(Some((nova, mudou)))
    }

    fn decodificar_no(&self, n: &No, base: Option<&[u8]>) -> Result<Vec<u8>> {
        let texto = if n.cheia {
            self.codec.descomprimir(&n.dados, n.tamanho as usize)?
        } else {
            let base = base.ok_or_else(|| Erro::Corrompido("diferença sem base".into()))?;
            self.codec
                .descomprimir_com_base(&n.dados, base, n.tamanho as usize)?
        };
        if impressao_de_bytes(&texto) != n.hash {
            return Err(Erro::Integridade {
                nota: "árvore de desfazer".into(),
                versao: 0,
            });
        }
        Ok(texto)
    }
}

/// Um filho foi processado: o pai libera o texto quando o último filho passou.
fn liberar(restam: &mut HashMap<u32, usize>, textos: &mut HashMap<u32, Vec<u8>>, pai: u32) {
    if let Some(r) = restam.get_mut(&pai) {
        *r = r.saturating_sub(1);
        if *r == 0 {
            textos.remove(&pai);
        }
    }
}

fn liberar_limpo(restam: &HashMap<u32, usize>, textos: &mut HashMap<u32, Vec<u8>>, pai: u32) {
    if restam.get(&pai).copied().unwrap_or(0) == 0 {
        textos.remove(&pai);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn resumo_mostra_o_que_entrou() {
        assert_eq!(
            resumo_da_mudanca("Olá mundo.", "Olá mundo. Tudo bem?"),
            "Tudo bem?"
        );
        assert_eq!(resumo_da_mudanca("", "primeira linha"), "primeira linha");
    }

    #[test]
    fn resumo_de_apagar_e_de_espacos() {
        assert_eq!(
            resumo_da_mudanca("uma frase inteira aqui", "uma aqui"),
            "apagou: frase inteira"
        );
        assert_eq!(resumo_da_mudanca("a b", "a  b"), "ajuste de espaços");
    }

    #[test]
    fn resumo_longo_e_cortado_e_respeita_unicode() {
        let longo = "ação ".repeat(40);
        let r = resumo_da_mudanca("", &longo);
        assert!(r.chars().count() <= RESUMO_MAXIMO + 1, "{r}");
        assert!(r.ends_with('…'));
    }

    #[test]
    fn registro_do_no_ida_e_volta() {
        let n = No {
            pai: 7,
            quando_ms: 1_763_000_000_123,
            tamanho: 99,
            hash: [3; 16],
            cheia: false,
            dist: 5,
            resumo: "é um resumo".into(),
            dados: vec![1, 2, 3, 4],
        };
        assert_eq!(No::decodificar(&n.codificar()).unwrap(), n);
        let b = n.codificar();
        for i in 0..FIXO_DO_NO {
            assert!(No::decodificar(&b[..i]).is_err(), "i={i}");
        }
        let c = Cabecalho {
            atual: 3,
            proximo: 9,
            passos: 8,
            bytes: 1234,
            criada_ms: 5,
            arquivada_ms: 0,
        };
        assert_eq!(Cabecalho::decodificar(&c.codificar()).unwrap(), c);
    }
}
