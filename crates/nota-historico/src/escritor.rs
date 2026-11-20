//! H4, "para quem escreve": achar o que apaguei e o histórico de um trecho. Tudo puro (sem
//! banco) sobre os textos das versões do H1; [`Historico::achar_apagados`] e
//! [`Historico::historico_do_trecho`] só leem as versões em lote e chamam estas funções.
//!
//! Nada é guardado a mais: o que sai do histórico (`apagar_texto_do_historico`) não aparece aqui.

use crate::diff::{Tipo, Trecho, diff_palavras};
use crate::erro::Result;
use crate::registro::{NotaId, VersaoId, VersaoInfo};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::SystemTime;

/// Quantas versões (as mais recentes) entram nas buscas: a leitura é uma passada só, mas notas com
/// dezenas de milhares de versões não precisam ser lidas inteiras.
pub const VERSOES_MAXIMAS: usize = 1_500;
/// Quantos trechos apagados a lista guarda (os mais recentes).
const APAGADOS_MAXIMOS: usize = 3_000;
/// Um trecho menor que isto (em letras e números) não vale como "apagado".
const MINIMO_DE_LETRAS: usize = 8;

/// Um trecho que existiu em alguma versão e não está no texto de agora.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Apagado {
    /// O texto como estava na última versão que o tinha.
    pub trecho: String,
    /// A primeira versão que o tinha.
    pub existiu_de: SystemTime,
    /// A última versão que o tinha.
    pub existiu_ate: SystemTime,
    /// A primeira versão sem ele (depois da última com ele). `None`: sumiu só no texto de agora,
    /// que ainda não virou versão.
    pub apagado_em: Option<SystemTime>,
    pub ultima_versao: VersaoId,
    /// Em quantas versões ele aparecia.
    pub versoes: usize,
}

/// Uma mudança do trecho selecionado, numa versão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MudancaDoTrecho {
    /// `None`: é o texto de agora, que ainda não é uma versão.
    pub versao: Option<VersaoId>,
    pub quando: Option<SystemTime>,
    pub nome: Option<String>,
    /// O texto daquela faixa nessa versão.
    pub texto: String,
    /// A faixa não existia na versão anterior (nasceu aqui).
    pub nasceu: bool,
}

/// Espaços repetidos e quebras de linha viram um espaço só.
pub fn normalizar(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Para buscar sem se importar com maiúsculas e acentos.
pub fn dobrar(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            outro => outro,
        })
        .collect()
}

/// Tira o que marca a linha (título, citação, lista, caixa de seleção, número) e deixa o texto.
fn sem_marcadores(s: &str) -> &str {
    let mut resto = s.trim_start();
    loop {
        let antes = resto;
        resto = resto
            .trim_start_matches(['#', '>', '-', '*', '+'])
            .trim_start();
        for caixa in ["[ ]", "[x]", "[X]"] {
            if let Some(r) = resto.strip_prefix(caixa) {
                resto = r.trim_start();
            }
        }
        let digitos = resto.chars().take_while(char::is_ascii_digit).count();
        if digitos > 0
            && let Some(r) = resto[digitos..]
                .strip_prefix('.')
                .or_else(|| resto[digitos..].strip_prefix(')'))
        {
            resto = r.trim_start();
        }
        if resto == antes {
            return resto.trim_end();
        }
    }
}

fn fecha_frase(c: char) -> bool {
    matches!(
        c,
        '.' | '!' | '?' | '…' | '"' | '”' | '\'' | ')' | ']' | '»'
    )
}

fn abre_frase(c: char) -> bool {
    c.is_uppercase()
        || c.is_numeric()
        || matches!(c, '"' | '“' | '«' | '(' | '¿' | '¡' | '-' | '*' | '#' | '[')
}

/// Corta uma linha em frases: depois de `.`, `!`, `?` ou `…` seguidos de espaço e de uma letra
/// maiúscula, número ou abertura.
fn frases(linha: &str) -> Vec<&str> {
    let mut saida = Vec::new();
    let mut inicio = 0;
    let chars: Vec<(usize, char)> = linha.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        if matches!(chars[i].1, '.' | '!' | '?' | '…') {
            let mut j = i + 1;
            while j < chars.len() && fecha_frase(chars[j].1) {
                j += 1;
            }
            let fim = chars.get(j).map_or(linha.len(), |(b, _)| *b);
            let mut k = j;
            while k < chars.len() && chars[k].1.is_whitespace() {
                k += 1;
            }
            if k > j && chars.get(k).is_none_or(|(_, n)| abre_frase(*n)) {
                saida.push(&linha[inicio..fim]);
                inicio = chars.get(k).map_or(linha.len(), |(b, _)| *b);
                i = k;
                continue;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if inicio < linha.len() {
        saida.push(&linha[inicio..]);
    }
    saida
}

/// As unidades de um texto: cada frase de cada linha, sem marcadores, com palavras suficientes.
pub fn unidades(texto: &str) -> Vec<String> {
    let mut saida = Vec::new();
    for linha in texto.lines() {
        let linha = linha.trim();
        if linha.is_empty() {
            continue;
        }
        for frase in frases(linha) {
            let u = sem_marcadores(frase.trim());
            let letras = u.chars().filter(|c| c.is_alphanumeric()).count();
            if letras >= MINIMO_DE_LETRAS && u.split_whitespace().count() >= 2 {
                saida.push(u.to_owned());
            }
        }
    }
    saida
}

struct Acumulado {
    texto: String,
    primeira: usize,
    ultima: usize,
    quantas: usize,
}

/// Os trechos que existiram em alguma das `versoes` (da mais antiga para a mais nova) e não estão
/// em `atual`. Um trecho contido em outro trecho apagado maior sai da lista. O mais recentemente
/// apagado vem primeiro.
pub fn achar_apagados(versoes: &[(VersaoInfo, String)], atual: &str) -> Vec<Apagado> {
    let atual_norm = normalizar(atual);
    let mut mapa: HashMap<String, Acumulado> = HashMap::new();
    for (i, (_, texto)) in versoes.iter().enumerate() {
        let mut desta: HashSet<String> = HashSet::new();
        for u in unidades(texto) {
            let chave = normalizar(&u);
            if !desta.insert(chave.clone()) {
                continue;
            }
            let a = mapa.entry(chave).or_insert_with(|| Acumulado {
                texto: String::new(),
                primeira: i,
                ultima: i,
                quantas: 0,
            });
            a.texto = u;
            a.ultima = i;
            a.quantas += 1;
        }
    }
    let mut candidatos: Vec<(String, Acumulado)> = mapa
        .into_iter()
        .filter(|(chave, _)| !atual_norm.contains(chave.as_str()))
        .collect();
    // Os mais recentes primeiro; se passar do teto, os mais antigos saem.
    candidatos.sort_by(|a, b| b.1.ultima.cmp(&a.1.ultima).then(a.0.cmp(&b.0)));
    candidatos.truncate(APAGADOS_MAXIMOS);
    // Um trecho dentro de outro apagado maior não vale à parte: valem os maiores.
    let mut por_tamanho: Vec<usize> = (0..candidatos.len()).collect();
    por_tamanho.sort_by_key(|&i| std::cmp::Reverse(candidatos[i].0.len()));
    let mut mantidos: Vec<usize> = Vec::new();
    for i in por_tamanho {
        if !mantidos
            .iter()
            .any(|&m| candidatos[m].0.contains(candidatos[i].0.as_str()))
        {
            mantidos.push(i);
        }
    }
    mantidos.sort_unstable();
    mantidos
        .into_iter()
        .map(|i| {
            let (_, a) = &candidatos[i];
            Apagado {
                trecho: a.texto.clone(),
                existiu_de: versoes[a.primeira].0.quando,
                existiu_ate: versoes[a.ultima].0.quando,
                apagado_em: versoes.get(a.ultima + 1).map(|(v, _)| v.quando),
                ultima_versao: versoes[a.ultima].0.id,
                versoes: a.quantas,
            }
        })
        .collect()
}

/// Os índices (em `apagados`) dos trechos que têm todas as palavras de `busca`, sem diferença de
/// maiúsculas nem de acentos. Busca vazia: todos.
pub fn filtrar_apagados(apagados: &[Apagado], busca: &str) -> Vec<usize> {
    let palavras: Vec<String> = dobrar(busca)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    apagados
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            let dobrado = dobrar(&a.trecho);
            palavras.iter().all(|p| dobrado.contains(p.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

struct Segmento {
    a: Range<usize>,
    b: Range<usize>,
    igual: bool,
}

/// Junta os trechos seguidos que mudaram num só (o que saiu e o que entrou no lugar).
fn segmentos(trechos: &[Trecho]) -> Vec<Segmento> {
    let (mut a, mut b) = (0usize, 0usize);
    let mut saida: Vec<Segmento> = Vec::new();
    for t in trechos {
        let n = t.texto.len();
        let (da, db, igual) = match t.tipo {
            Tipo::Igual => (n, n, true),
            Tipo::Entrou => (n, 0, false),
            Tipo::Saiu => (0, n, false),
        };
        match saida.last_mut() {
            Some(s) if !s.igual && !igual => {
                s.a.end += da;
                s.b.end += db;
            }
            _ => saida.push(Segmento {
                a: a..a + da,
                b: b..b + db,
                igual,
            }),
        }
        a += da;
        b += db;
    }
    saida
}

/// Leva uma faixa do texto de depois para o texto de antes. Uma faixa que toca um trecho que mudou
/// o inclui inteiro (o que saiu naquele lugar entra na faixa de antes).
pub fn faixa_de_antes(trechos: &[Trecho], faixa: Range<usize>) -> Range<usize> {
    let segs = segmentos(trechos);
    let total_b = segs.last().map_or(0, |s| s.b.end);
    let ponto_inicial = |pos: usize| -> usize {
        for s in &segs {
            if s.a.start <= pos && pos < s.a.end {
                return if s.igual {
                    s.b.start + (pos - s.a.start)
                } else {
                    s.b.start
                };
            }
        }
        total_b
    };
    let ponto_final = |pos: usize| -> usize {
        for s in &segs {
            if s.a.start < pos && pos <= s.a.end {
                return if s.igual {
                    s.b.start + (pos - s.a.start)
                } else {
                    s.b.end
                };
            }
        }
        0
    };
    let inicio = ponto_inicial(faixa.start);
    let fim = ponto_final(faixa.end).max(inicio);
    inicio..fim
}

/// As versões em que a faixa `selecao` de `atual` (em bytes) mudou, da mais nova para a mais
/// antiga. A faixa é levada para trás, de versão em versão, pela diferença por palavra.
pub fn historico_do_trecho(
    versoes: &[(VersaoInfo, String)],
    atual: &str,
    selecao: Range<usize>,
) -> Vec<MudancaDoTrecho> {
    if selecao.start >= selecao.end
        || selecao.end > atual.len()
        || !atual.is_char_boundary(selecao.start)
        || !atual.is_char_boundary(selecao.end)
    {
        return Vec::new();
    }
    let corte = versoes.len().saturating_sub(VERSOES_MAXIMAS);
    let versoes = &versoes[corte..];
    // Os estados, do mais antigo ao de agora (que só entra se ainda não é uma versão).
    let mut estados: Vec<(Option<&VersaoInfo>, &str)> =
        versoes.iter().map(|(v, t)| (Some(v), t.as_str())).collect();
    if versoes.last().is_none_or(|(_, t)| t != atual) {
        estados.push((None, atual));
    }
    let n = estados.len();
    let mut faixas: Vec<Range<usize>> = vec![0..0; n];
    faixas[n - 1] = selecao;
    for i in (1..n).rev() {
        let d = diff_palavras(estados[i - 1].1, estados[i].1);
        faixas[i - 1] = faixa_de_antes(&d, faixas[i].clone());
    }
    let texto_de = |i: usize| -> &str { estados[i].1.get(faixas[i].clone()).unwrap_or("") };
    let mut saida = Vec::new();
    for i in (0..n).rev() {
        let aqui = texto_de(i);
        let antes = if i == 0 { "" } else { texto_de(i - 1) };
        if aqui.trim().is_empty() || aqui == antes {
            continue;
        }
        let (versao, quando, nome) = match estados[i].0 {
            Some(v) => (Some(v.id), Some(v.quando), v.nome.clone()),
            None => (None, None, None),
        };
        saida.push(MudancaDoTrecho {
            versao,
            quando,
            nome,
            texto: aqui.to_owned(),
            nasceu: antes.trim().is_empty(),
        });
    }
    saida
}

impl crate::Historico {
    /// Os trechos que existiram nas versões da nota e não estão em `atual` (ver
    /// [`achar_apagados`]). Lê as versões em lote: rode numa linha de fundo.
    pub fn achar_apagados(&self, nota: &NotaId, atual: &str) -> Result<Vec<Apagado>> {
        let versoes = self.textos_da_nota(nota, VERSOES_MAXIMAS)?;
        Ok(achar_apagados(&versoes, atual))
    }

    /// As versões em que a faixa `selecao` (bytes) de `atual` mudou (ver [`historico_do_trecho`]).
    pub fn historico_do_trecho(
        &self,
        nota: &NotaId,
        atual: &str,
        selecao: Range<usize>,
    ) -> Result<Vec<MudancaDoTrecho>> {
        let versoes = self.textos_da_nota(nota, VERSOES_MAXIMAS)?;
        Ok(historico_do_trecho(&versoes, atual, selecao))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::registro::Origem;
    use std::time::{Duration, UNIX_EPOCH};

    fn v(id: u64, seg: u64, texto: &str) -> (VersaoInfo, String) {
        (
            VersaoInfo {
                id: VersaoId(id),
                quando: UNIX_EPOCH + Duration::from_secs(seg),
                origem: Origem::Editor,
                tamanho: texto.len(),
                nome: None,
                impressao: crate::registro::impressao_do_texto(texto),
            },
            texto.to_owned(),
        )
    }

    #[test]
    fn frases_cortam_no_ponto_seguido_de_maiuscula() {
        assert_eq!(
            frases("Primeira frase. Segunda frase! E uma terceira? sim, continua."),
            vec![
                "Primeira frase.",
                "Segunda frase!",
                "E uma terceira? sim, continua."
            ]
        );
        // "3.5" e "Sr. silva" não cortam.
        assert_eq!(
            frases("Custa 3.5 reais, disse o Sr. silva."),
            vec!["Custa 3.5 reais, disse o Sr. silva."]
        );
    }

    #[test]
    fn unidades_tiram_marcadores_e_ignoram_o_curto() {
        let t =
            "# Título da nota\n\n- [ ] Comprar leite e pão\n1. Ligar para a Maria hoje\nok\n---\n";
        assert_eq!(
            unidades(t),
            vec![
                "Título da nota",
                "Comprar leite e pão",
                "Ligar para a Maria hoje"
            ]
        );
    }

    #[test]
    fn acha_a_frase_que_saiu_e_ignora_a_que_ficou() {
        let versoes = vec![
            v(1, 100, "Escrevi isto primeiro. Esta frase vai sumir logo."),
            v(
                2,
                200,
                "Escrevi isto primeiro. Esta frase vai sumir logo. Fica sempre.",
            ),
            v(3, 300, "Escrevi isto primeiro. Fica sempre."),
        ];
        let atual = "Escrevi isto primeiro. Fica sempre. Mais uma coisa.";
        let a = achar_apagados(&versoes, atual);
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].trecho, "Esta frase vai sumir logo.");
        assert_eq!(a[0].existiu_de, versoes[0].0.quando);
        assert_eq!(a[0].existiu_ate, versoes[1].0.quando);
        assert_eq!(a[0].apagado_em, Some(versoes[2].0.quando));
        assert_eq!(a[0].ultima_versao, VersaoId(2));
        assert_eq!(a[0].versoes, 2);
    }

    #[test]
    fn trecho_so_apagado_no_texto_de_agora_nao_tem_data_de_apagado() {
        let versoes = vec![v(1, 100, "Uma frase que ainda existe na última versão.")];
        let a = achar_apagados(&versoes, "outra coisa inteira agora");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].apagado_em, None);
    }

    #[test]
    fn espacos_e_quebras_nao_contam_como_apagado() {
        let versoes = vec![v(1, 100, "Um texto com   espaços e\nquebras de linha.")];
        assert!(achar_apagados(&versoes, "Um texto com espaços e quebras de linha.").is_empty());
    }

    #[test]
    fn digitacao_pela_metade_nao_polui_a_lista() {
        let versoes = vec![
            v(1, 100, "Eu gosto de ga"),
            v(2, 200, "Eu gosto de gatos e cachorros."),
        ];
        // O texto atual tem a frase completa: nada foi apagado.
        assert!(achar_apagados(&versoes, "Eu gosto de gatos e cachorros.").is_empty());
        // Se a frase completa some, só ela aparece (a metade está contida nela).
        let a = achar_apagados(&versoes, "Nada a ver com isto aqui.");
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].trecho, "Eu gosto de gatos e cachorros.");
    }

    #[test]
    fn busca_ignora_acento_e_caixa_e_exige_todas_as_palavras() {
        let versoes = vec![v(
            1,
            100,
            "A cafeteira antiga ficava na cozinha.\nO violão estava no sótão da casa.\n",
        )];
        let a = achar_apagados(&versoes, "nada");
        assert_eq!(a.len(), 2);
        assert_eq!(filtrar_apagados(&a, "").len(), 2);
        let i = filtrar_apagados(&a, "VIOLAO sotao");
        assert_eq!(i.len(), 1);
        assert!(a[i[0]].trecho.contains("violão"));
        assert!(filtrar_apagados(&a, "violão cozinha").is_empty());
    }

    #[test]
    fn faixa_de_antes_acompanha_o_que_mudou() {
        let antes = "um dois três quatro cinco";
        let depois = "um dois TRÊS quatro cinco";
        let d = diff_palavras(antes, depois);
        // Selecionar "quatro" (depois) leva ao mesmo "quatro" (antes).
        let ini = depois.find("quatro").unwrap();
        let f = faixa_de_antes(&d, ini..ini + 6);
        assert_eq!(&antes[f], "quatro");
        // Selecionar "TRÊS" leva ao "três" que saiu.
        let ini = depois.find("TRÊS").unwrap();
        let f = faixa_de_antes(&d, ini..ini + "TRÊS".len());
        assert_eq!(&antes[f], "três");
    }

    #[test]
    fn historico_do_trecho_lista_so_as_versoes_em_que_a_frase_mudou() {
        let versoes = vec![
            v(
                1,
                100,
                "# Capítulo\n\nO velho entrou no bar.\n\nFim da cena.\n",
            ),
            v(
                2,
                200,
                "# Capítulo\n\nO velho entrou no bar.\n\nFim da cena. Nota solta.\n",
            ),
            v(
                3,
                300,
                "# Capítulo\n\nO velho cansado entrou no bar.\n\nFim da cena. Nota solta.\n",
            ),
            v(
                4,
                400,
                "# Capítulo\n\nO velho cansado entrou no bar vazio.\n\nFim da cena. Nota solta.\n",
            ),
            v(
                5,
                500,
                "# Capítulo\n\nO velho cansado entrou no bar vazio.\n\nFim da cena. Nota solta e longa.\n",
            ),
        ];
        let atual = &versoes[4].1;
        let ini = atual.find("O velho").unwrap();
        let fim = ini + "O velho cansado entrou no bar vazio.".len();
        let h = historico_do_trecho(&versoes, atual, ini..fim);
        let textos: Vec<&str> = h.iter().map(|m| m.texto.as_str()).collect();
        assert_eq!(
            textos,
            [
                "O velho cansado entrou no bar vazio.",
                "O velho cansado entrou no bar.",
                "O velho entrou no bar."
            ],
            "{h:#?}"
        );
        assert_eq!(h[0].versao, Some(VersaoId(4)));
        assert_eq!(h[1].versao, Some(VersaoId(3)));
        assert_eq!(h[2].versao, Some(VersaoId(1)));
        assert!(h[2].nasceu && !h[0].nasceu);
    }

    #[test]
    fn historico_do_trecho_inclui_o_texto_de_agora_se_ainda_nao_e_versao() {
        let versoes = vec![v(1, 100, "Uma frase simples aqui.")];
        let atual = "Uma frase simples e mudada aqui.";
        let h = historico_do_trecho(&versoes, atual, 0..atual.len());
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].versao, None);
        assert_eq!(h[1].versao, Some(VersaoId(1)));
    }

    #[test]
    fn selecao_vazia_ou_fora_do_texto_nao_da_nada() {
        let versoes = vec![v(1, 100, "áé")];
        assert!(historico_do_trecho(&versoes, "áé", 0..0).is_empty());
        assert!(historico_do_trecho(&versoes, "áé", 0..9).is_empty());
        assert!(historico_do_trecho(&versoes, "áé", 1..3).is_empty());
    }
}
