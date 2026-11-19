//! Gerador sintético compartilhado pelos testes e pela medição: texto em português e edições
//! aleatórias de ~50 caracteres (inserir, apagar, trocar), com semente fixa.
#![allow(dead_code)]

/// xorshift64*: pequeno, determinístico, sem dependência.
pub struct Sorteio(pub u64);

impl Sorteio {
    pub fn novo(semente: u64) -> Self {
        Self(semente.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn proximo(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn ate(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.proximo() % n as u64) as usize
        }
    }
}

const PALAVRAS: &[&str] = &[
    "a",
    "o",
    "de",
    "que",
    "em",
    "um",
    "para",
    "com",
    "não",
    "uma",
    "os",
    "no",
    "se",
    "na",
    "por",
    "mais",
    "as",
    "dos",
    "como",
    "mas",
    "ao",
    "ele",
    "das",
    "à",
    "seu",
    "sua",
    "ou",
    "quando",
    "muito",
    "nos",
    "já",
    "eu",
    "também",
    "só",
    "pelo",
    "pela",
    "até",
    "isso",
    "ela",
    "entre",
    "depois",
    "sem",
    "mesmo",
    "aos",
    "ter",
    "seus",
    "quem",
    "nas",
    "me",
    "esse",
    "eles",
    "você",
    "essa",
    "num",
    "nem",
    "suas",
    "meu",
    "às",
    "minha",
    "numa",
    "pelos",
    "elas",
    "havia",
    "seja",
    "qual",
    "será",
    "nós",
    "tenho",
    "lhe",
    "deles",
    "essas",
    "esses",
    "pelas",
    "este",
    "fosse",
    "dele",
    "tu",
    "te",
    "vocês",
    "vos",
    "lhes",
    "meus",
    "minhas",
    "teu",
    "tua",
    "teus",
    "tuas",
    "nosso",
    "nossa",
    "nossos",
    "nossas",
    "dela",
    "delas",
    "esta",
    "estes",
    "estas",
    "aquele",
    "aquela",
    "aqueles",
    "aquelas",
    "isto",
    "aquilo",
    "estou",
    "está",
    "estamos",
    "estão",
    "reunião",
    "projeto",
    "história",
    "capítulo",
    "personagem",
    "cidade",
    "manhã",
    "coração",
    "ideia",
    "anotação",
    "decisão",
    "informação",
    "avaliação",
    "organização",
    "solução",
    "caminho",
    "janela",
    "memória",
    "viagem",
    "escrever",
    "revisar",
    "lembrar",
    "começar",
    "terminar",
    "precisamos",
    "podemos",
    "devemos",
    "talvez",
    "sempre",
    "nunca",
    "agora",
    "ontem",
    "amanhã",
    "importante",
    "diferente",
    "possível",
    "necessário",
    "simples",
    "difícil",
];

/// `n` caracteres (aprox.) de texto em português, com frases e parágrafos.
pub fn texto(s: &mut Sorteio, n: usize) -> String {
    let mut t = String::new();
    let mut na_frase = 0;
    while t.chars().count() < n {
        let p = PALAVRAS[s.ate(PALAVRAS.len())];
        if na_frase == 0 {
            let mut c = p.chars();
            if let Some(f) = c.next() {
                t.extend(f.to_uppercase());
                t.push_str(c.as_str());
            }
        } else {
            t.push_str(p);
        }
        na_frase += 1;
        if na_frase > 6 && s.ate(5) == 0 {
            t.push('.');
            if s.ate(6) == 0 {
                t.push_str("\n\n");
            } else {
                t.push(' ');
            }
            na_frase = 0;
        } else if s.ate(12) == 0 {
            t.push_str(", ");
        } else {
            t.push(' ');
        }
    }
    t
}

fn byte_de_char(t: &str, c: usize) -> usize {
    t.char_indices().nth(c).map(|(i, _)| i).unwrap_or(t.len())
}

/// Aplica uma edição de ~`tam` caracteres: inserir, apagar ou trocar, em posição aleatória.
pub fn editar(s: &mut Sorteio, t: &mut String, tam: usize) {
    let total = t.chars().count();
    let ini = s.ate(total.saturating_sub(1).max(1));
    let tipo = s.ate(3);
    let mut novo = texto(s, tam);
    let corte = byte_de_char(&novo, tam);
    novo.truncate(corte);
    let fim = (ini + tam).min(total);
    let (b0, b1) = (byte_de_char(t, ini), byte_de_char(t, fim));
    match tipo {
        0 => t.insert_str(b0, &novo),
        1 => t.replace_range(b0..b1, ""),
        _ => t.replace_range(b0..b1, &novo),
    }
    // Mantém a nota por volta de 5 KB: se encolheu demais, cresce.
    if t.chars().count() + 400 < total {
        let extra = texto(s, 100);
        t.push_str(&extra);
    }
}
