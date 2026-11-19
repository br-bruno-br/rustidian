//! Histórico das notas no app (fase H1): o `nota-historico` atrás de uma linha de fundo.
//!
//! Tudo o que toca o banco (`redb`), o zstd ou o blake3 de uma nota roda numa thread só, dona do
//! arquivo `<cofre>/.rustidian/historico.redb`. O quadro do editor só manda mensagens (um `String`
//! quando o texto mudou) e lê respostas; nunca espera o disco. Um escritor por arquivo: se outra
//! instância do app já o abriu, o histórico fica indisponível nesta, a interface segue e o aviso
//! vem uma vez.
//!
//! # Quem decide quando gravar
//!
//! O [`Agrupador`] (do crate) mora na thread de fundo, uma por nota aberta. A interface manda
//! `editou` (o texto mudou), `salvou`, `carregou` (a nota foi relida do disco) e `saiu` (trocou de
//! aba); a thread conta o tempo sozinha (`Pausa`) e grava no fim de frase seguido de pausa, no
//! Enter, depois de ~2 s parado, a cada ~300 caracteres, ao salvar, ao trocar de nota e ao fechar.
//! Assim o blake3 que o agrupador calcula a cada edição também sai do quadro.
//!
//! # Identidade da nota
//!
//! O id é o caminho relativo no cofre, com `/` em qualquer sistema. Ele é "estável" porque o app
//! avisa cada renomeação e movimento ([`Historico::renomear`]), inclusive de pastas inteiras, e o
//! crate passa as versões para o id novo sem tocar nelas.
//!
//! # O que continua valendo
//!
//! O `.md` é a verdade. Se o histórico não abrir ou corromper, nada muda para a nota. Nada aqui
//! é cifrado e o sync ignora `.rustidian/` (`nota-sync`, `caminhos.rs`).

use crate::{
    paths,
    recovery::History as Legado,
    vault::{Document, Vault},
};
use nota_historico::{
    Agrupador, Evento, Historico as Banco, NotaId, PoliticaRetencao, impressao_do_texto,
};
pub use nota_historico::{Origem, Ritmo, Tipo, Trecho, VersaoId, VersaoInfo, diff_palavras};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub type Resultado<T> = Result<T, String>;

/// Notas até este tamanho usam o quadro-chave padrão do crate (128), medido com 5 KB.
const NOTA_PEQUENA: usize = 200 * 1024;
/// Acima disso o pior caso de abrir uma versão cresce com N x tamanho: quadro-chave mais frequente.
const NOTA_GRANDE: usize = 1024 * 1024;
/// Quanto esperar o disco depois de uma gravação com `persistir` (a régua do briefing: no máximo 1 s).
const PERSISTIR_APOS: Duration = Duration::from_secs(1);
const PODAR_A_CADA: Duration = Duration::from_secs(3600);

/// O id estável de uma nota no histórico: o caminho relativo, com `/`.
pub fn id_da_nota(relativo: &Path) -> NotaId {
    NotaId::novo(paths::slashed(relativo))
}

/// De quantas em quantas versões há um quadro-chave, pelo tamanho da nota.
pub fn quadro_chave_para(bytes: usize) -> u64 {
    if bytes > NOTA_GRANDE {
        32
    } else if bytes > NOTA_PEQUENA {
        48
    } else {
        128
    }
}

/// Onde fica o arquivo do histórico de um cofre.
pub fn arquivo_do_cofre(raiz: &Path) -> PathBuf {
    raiz.join(".rustidian").join("historico.redb")
}

fn marca_de_migracao(raiz: &Path) -> PathBuf {
    raiz.join(".rustidian").join("historico-migrado.txt")
}

enum Pedido {
    Abriu(PathBuf, String),
    Editou(PathBuf, String),
    Salvou(PathBuf),
    Carregou(PathBuf, String),
    Saiu(PathBuf),
    Fechou(PathBuf),
    Gravar(PathBuf, String, Origem, Option<Sender<Resultado<bool>>>),
    GravarEm(PathBuf, String, Origem, SystemTime),
    Renomear(PathBuf, PathBuf),
    Configurar(bool),
    Limpar(Sender<Resultado<()>>),
    ApagarTexto(PathBuf, String, Sender<Resultado<usize>>),
    Versoes(PathBuf, Sender<Resultado<Vec<VersaoInfo>>>),
    Ler(PathBuf, VersaoId, Sender<Resultado<String>>),
    Notas(Sender<Resultado<Vec<PathBuf>>>),
    Fechar,
}

/// A mão do app no histórico. Só a thread de fundo toca o banco; os métodos daqui não esperam.
pub struct Historico {
    tx: Sender<Pedido>,
    pronto: Receiver<()>,
    juntar: Option<JoinHandle<()>>,
    habilitado: bool,
    /// Avisos para mostrar ao usuário (o histórico indisponível, por exemplo).
    pub avisos: Receiver<String>,
    /// Um caminho a cada versão gravada: a tela de versões se atualiza.
    pub gravou: Receiver<PathBuf>,
}

/// Uma cópia barata do canal de pedidos, para as threads de fundo da tela de versões. Os métodos
/// daqui **esperam** a resposta: nunca os chame de dentro do quadro da interface.
#[derive(Clone)]
pub struct Leitor {
    tx: Sender<Pedido>,
}

impl Historico {
    /// Sobe a thread de fundo. O arquivo só abre quando o primeiro pedido precisa dele.
    pub fn iniciar(
        raiz: &Path,
        habilitado: bool,
        ritmo: Ritmo,
        acordar: impl Fn() + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let (avisos_tx, avisos) = mpsc::channel();
        let (gravou_tx, gravou) = mpsc::channel();
        let (pronto_tx, pronto) = mpsc::channel();
        let mut fundo = Fundo {
            rx,
            raiz: raiz.to_owned(),
            ritmo,
            habilitado,
            banco: None,
            indisponivel: None,
            notas: HashMap::new(),
            inicio: Instant::now(),
            gravacao_pendente: None,
            proxima_poda: Instant::now(),
            avisos: avisos_tx,
            ultimo_aviso: None,
            gravou: gravou_tx,
            acordar: Box::new(acordar),
        };
        let juntar = std::thread::Builder::new()
            .name("historico".into())
            .spawn(move || {
                fundo.correr();
                let _ = pronto_tx.send(());
            })
            .ok();
        Self {
            tx,
            pronto,
            juntar,
            habilitado,
            avisos,
            gravou,
        }
    }

    pub fn leitor(&self) -> Leitor {
        Leitor {
            tx: self.tx.clone(),
        }
    }

    /// A nota apareceu numa aba, com o texto que está no disco. Sem histórico ainda, vira a
    /// primeira versão; com o texto diferente da última versão, vira uma versão "mudou fora".
    pub fn abriu(&self, nota: &Path, texto: &str) {
        let _ = self
            .tx
            .send(Pedido::Abriu(nota.to_owned(), texto.to_owned()));
    }

    /// O texto da nota mudou na tela (digitação, colar, desfazer). Mande só quando mudou.
    pub fn editou(&self, nota: &Path, texto: &str) {
        let _ = self
            .tx
            .send(Pedido::Editou(nota.to_owned(), texto.to_owned()));
    }

    /// A nota foi salva: grava o que sobrou da digitação.
    pub fn salvou(&self, nota: &Path) {
        let _ = self.tx.send(Pedido::Salvou(nota.to_owned()));
    }

    /// A nota foi relida do disco (mudança de fora, restauração): o texto novo é a base, sem
    /// virar versão por conta da digitação. A versão "de fora" entra por [`Historico::gravar`].
    pub fn carregou(&self, nota: &Path, texto: &str) {
        let _ = self
            .tx
            .send(Pedido::Carregou(nota.to_owned(), texto.to_owned()));
    }

    /// A nota deixou de ser a ativa: grava o que sobrou, a aba continua aberta.
    pub fn saiu(&self, nota: &Path) {
        let _ = self.tx.send(Pedido::Saiu(nota.to_owned()));
    }

    /// A aba da nota fechou: grava o que sobrou e esquece o estado da nota.
    pub fn fechou(&self, nota: &Path) {
        let _ = self.tx.send(Pedido::Fechou(nota.to_owned()));
    }

    /// Grava uma versão agora (mudança de fora, sync, restauração). Texto igual ao da última
    /// versão não grava.
    pub fn gravar(&self, nota: &Path, texto: &str, origem: Origem) {
        let _ = self.tx.send(Pedido::Gravar(
            nota.to_owned(),
            texto.to_owned(),
            origem,
            None,
        ));
    }

    /// Como [`Historico::gravar`], com a hora da versão dada (importações; os prints de exemplo).
    pub fn gravar_em(&self, nota: &Path, texto: &str, origem: Origem, quando: SystemTime) {
        let _ = self.tx.send(Pedido::GravarEm(
            nota.to_owned(),
            texto.to_owned(),
            origem,
            quando,
        ));
    }

    /// Como [`Historico::gravar`], mas espera a resposta por no máximo `ate`. `Ok(None)` = ainda
    /// não respondeu (a gravação segue na fila, em ordem); `Err` = o histórico recusou.
    pub fn gravar_esperando(
        &self,
        nota: &Path,
        texto: &str,
        origem: Origem,
        ate: Duration,
    ) -> Resultado<Option<bool>> {
        let (resp, rx) = mpsc::channel();
        self.tx
            .send(Pedido::Gravar(
                nota.to_owned(),
                texto.to_owned(),
                origem,
                Some(resp),
            ))
            .map_err(|_| "O histórico local está fechado.".to_string())?;
        match rx.recv_timeout(ate) {
            Ok(r) => r.map(Some),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("O histórico local parou.".into()),
        }
    }

    /// A nota (ou a pasta) foi renomeada ou movida: o histórico vai junto.
    pub fn renomear(&self, de: &Path, para: &Path) {
        let _ = self
            .tx
            .send(Pedido::Renomear(de.to_owned(), para.to_owned()));
    }

    /// Liga ou desliga. Desligado: nada é gravado e o arquivo do histórico é apagado.
    pub fn configurar(&mut self, habilitado: bool) {
        if self.habilitado != habilitado {
            self.habilitado = habilitado;
            let _ = self.tx.send(Pedido::Configurar(habilitado));
        }
    }

    /// Apaga todo o histórico do cofre. A resposta chega quando acabar.
    pub fn limpar(&self) -> Resultado<Receiver<Resultado<()>>> {
        let (resp, rx) = mpsc::channel();
        self.tx
            .send(Pedido::Limpar(resp))
            .map_err(|_| "O histórico local está fechado.".to_string())?;
        Ok(rx)
    }

    /// Tira `trecho` de todas as versões da nota (uma senha colada por engano). Tire também do
    /// `.md`, senão a próxima versão o traz de volta. Devolve em quantas versões aparecia.
    pub fn apagar_texto(&self, nota: &Path, trecho: &str) -> Resultado<Receiver<Resultado<usize>>> {
        let (resp, rx) = mpsc::channel();
        self.tx
            .send(Pedido::ApagarTexto(
                nota.to_owned(),
                trecho.to_owned(),
                resp,
            ))
            .map_err(|_| "O histórico local está fechado.".to_string())?;
        Ok(rx)
    }
}

impl Drop for Historico {
    fn drop(&mut self) {
        let _ = self.tx.send(Pedido::Fechar);
        // Espera o fim da gravação pendente, mas não para sempre (uma compactação longa não
        // pode prender o fechamento da janela).
        if self.pronto.recv_timeout(Duration::from_secs(5)).is_ok()
            && let Some(juntar) = self.juntar.take()
        {
            let _ = juntar.join();
        }
    }
}

impl Leitor {
    fn perguntar<T>(&self, pedido: impl FnOnce(Sender<Resultado<T>>) -> Pedido) -> Resultado<T> {
        let (resp, rx) = mpsc::channel();
        self.tx
            .send(pedido(resp))
            .map_err(|_| "O histórico local está fechado.".to_string())?;
        rx.recv()
            .map_err(|_| "O histórico local parou antes de responder.".to_string())?
    }

    /// As versões da nota, da mais antiga para a mais nova.
    pub fn versoes(&self, nota: &Path) -> Resultado<Vec<VersaoInfo>> {
        self.perguntar(|r| Pedido::Versoes(nota.to_owned(), r))
    }

    pub fn ler(&self, nota: &Path, versao: VersaoId) -> Resultado<String> {
        self.perguntar(|r| Pedido::Ler(nota.to_owned(), versao, r))
    }

    /// As notas que têm histórico.
    pub fn notas(&self) -> Resultado<Vec<PathBuf>> {
        self.perguntar(Pedido::Notas)
    }
}

/// Restaura uma versão pelo caminho comum de salvar (com conferência de conflito). O texto que
/// estava salvo vira uma versão antes de ser trocado, e o texto restaurado vira uma versão nova
/// (origem restauração): nada se perde.
pub fn restaurar(
    historico: &Historico,
    vault: &mut Vault,
    esperado: &Document,
    texto: &str,
) -> Resultado<Document> {
    if esperado.dirty() && (esperado.saved_text().is_some() || !esperado.text.is_empty()) {
        return Err("Salve ou descarte as edições atuais antes de restaurar.".into());
    }
    if let Some(salvo) = esperado.saved_text() {
        // Se o fundo está ocupado (compactando), a gravação segue na fila, em ordem. Só um
        // erro de verdade (histórico indisponível) impede de trocar o arquivo.
        historico.gravar_esperando(
            &esperado.path,
            salvo,
            Origem::Editor,
            Duration::from_secs(2),
        )?;
    }
    let mut restaurado = esperado.clone();
    restaurado.text = texto.to_owned();
    vault.save(&mut restaurado)?;
    historico.gravar(&esperado.path, texto, Origem::Restauracao);
    Ok(restaurado)
}

// ------------------------------------------------------------------------------------ o fundo

struct Nota {
    agrupador: Agrupador,
    /// O último texto que a tela mandou.
    texto: String,
}

struct Fundo {
    rx: Receiver<Pedido>,
    raiz: PathBuf,
    ritmo: Ritmo,
    habilitado: bool,
    banco: Option<Banco>,
    /// Por que o banco não abriu; o aviso sai uma vez.
    indisponivel: Option<String>,
    notas: HashMap<PathBuf, Nota>,
    inicio: Instant,
    /// Desde quando há gravação sem `persistir`.
    gravacao_pendente: Option<Instant>,
    proxima_poda: Instant,
    avisos: Sender<String>,
    ultimo_aviso: Option<String>,
    gravou: Sender<PathBuf>,
    acordar: Box<dyn Fn() + Send>,
}

impl Fundo {
    fn correr(&mut self) {
        if !self.habilitado
            && let Err(erro) = self.apagar_arquivos()
        {
            self.avisar(erro);
        }
        loop {
            let espera = self.espera();
            match self.rx.recv_timeout(espera) {
                Ok(Pedido::Fechar) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(pedido) => self.tratar(pedido),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.pausas();
            self.manutencao();
        }
        self.encerrar();
    }

    fn agora(&self) -> Duration {
        self.inicio.elapsed()
    }

    /// Até quando dormir: o próximo prazo de um agrupador, a hora de persistir ou a de podar.
    fn espera(&self) -> Duration {
        let agora = self.agora();
        let mut espera = Duration::from_secs(60);
        for nota in self.notas.values() {
            if let Some(prazo) = nota.agrupador.proximo_prazo() {
                espera = espera.min(prazo.saturating_sub(agora));
            }
        }
        if let Some(desde) = self.gravacao_pendente {
            espera = espera.min((desde + PERSISTIR_APOS).saturating_duration_since(Instant::now()));
        }
        if self.habilitado && self.indisponivel.is_none() {
            espera = espera.min(self.proxima_poda.saturating_duration_since(Instant::now()));
        }
        espera.max(Duration::from_millis(1))
    }

    fn tratar(&mut self, pedido: Pedido) {
        match pedido {
            Pedido::Abriu(nota, texto) => self.abriu(nota, texto),
            Pedido::Editou(nota, texto) => {
                if !self.habilitado {
                    return;
                }
                let quando = self.agora();
                let ritmo = self.ritmo;
                let n = self.notas.entry(nota.clone()).or_insert_with(|| Nota {
                    agrupador: Agrupador::novo(ritmo),
                    texto: String::new(),
                });
                n.texto = texto;
                let grava = n
                    .agrupador
                    .evento(Evento::Editou {
                        quando,
                        texto: &n.texto,
                        cursor: None,
                    })
                    .is_some();
                if grava {
                    self.gravar_texto_da_nota(&nota, Origem::Editor);
                }
            }
            Pedido::Salvou(nota) => {
                self.evento_da_nota(&nota, |agora| Evento::Salvou { quando: agora });
            }
            Pedido::Carregou(nota, texto) => {
                if !self.habilitado {
                    return;
                }
                // A digitação que sobrou é do usuário: vira versão antes de o texto de fora entrar.
                self.evento_da_nota(&nota, |_| Evento::TrocouDeNota);
                let ritmo = self.ritmo;
                let n = self.notas.entry(nota).or_insert_with(|| Nota {
                    agrupador: Agrupador::novo(ritmo),
                    texto: String::new(),
                });
                n.agrupador.iniciar(&texto);
                n.texto = texto;
            }
            Pedido::Saiu(nota) => {
                self.evento_da_nota(&nota, |_| Evento::TrocouDeNota);
                if let Some(n) = self.notas.get_mut(&nota) {
                    n.agrupador.iniciar(&n.texto);
                }
            }
            Pedido::Fechou(nota) => {
                self.evento_da_nota(&nota, |_| Evento::Fechou);
                self.notas.remove(&nota);
            }
            Pedido::Gravar(nota, texto, origem, resp) => {
                let r = if self.habilitado {
                    self.gravar(&nota, &texto, origem)
                } else {
                    Ok(false)
                };
                if let Some(resp) = resp {
                    let _ = resp.send(r);
                }
            }
            Pedido::GravarEm(nota, texto, origem, quando) => {
                if self.habilitado {
                    let _ = self.gravar_em(&nota, &texto, origem, quando);
                }
            }
            Pedido::Renomear(de, para) => self.renomear(&de, &para),
            Pedido::Configurar(habilitado) => {
                self.habilitado = habilitado;
                if habilitado {
                    self.proxima_poda = Instant::now();
                } else {
                    self.notas.clear();
                    if let Err(erro) = self.apagar_arquivos() {
                        self.avisar(erro);
                    }
                }
            }
            Pedido::Limpar(resp) => {
                let r = self.apagar_arquivos();
                if r.is_ok() {
                    // Pode gravar de novo, num arquivo novo.
                    self.indisponivel = None;
                }
                let _ = resp.send(r);
                (self.acordar)();
            }
            Pedido::ApagarTexto(nota, trecho, resp) => {
                let r = self.banco().and_then(|b| {
                    b.apagar_texto_do_historico(&id_da_nota(&nota), &trecho)
                        .map_err(|e| e.to_string())
                });
                let _ = resp.send(r);
                (self.acordar)();
            }
            Pedido::Versoes(nota, resp) => {
                let r = self
                    .banco()
                    .and_then(|b| b.versoes(&id_da_nota(&nota)).map_err(|e| e.to_string()));
                let _ = resp.send(r);
            }
            Pedido::Ler(nota, versao, resp) => {
                let r = self.banco().and_then(|b| {
                    b.ler(&id_da_nota(&nota), versao)
                        .map_err(|e| format!("Esta versão não pôde ser lida: {e}"))
                });
                let _ = resp.send(r);
            }
            Pedido::Notas(resp) => {
                let r = self.banco().and_then(|b| {
                    b.notas()
                        .map(|ids| ids.iter().map(|id| PathBuf::from(id.como_str())).collect())
                        .map_err(|e| e.to_string())
                });
                let _ = resp.send(r);
            }
            Pedido::Fechar => {}
        }
    }

    fn abriu(&mut self, nota: PathBuf, texto: String) {
        if !self.habilitado {
            return;
        }
        // Já acompanhada (a aba foi renomeada ou reaberta): o estado e a digitação pendente ficam.
        if self.notas.contains_key(&nota) {
            return;
        }
        let mut agrupador = Agrupador::novo(self.ritmo);
        agrupador.iniciar(&texto);
        self.notas.insert(
            nota.clone(),
            Nota {
                agrupador,
                texto: texto.clone(),
            },
        );
        let id = id_da_nota(&nota);
        let origem = match self
            .banco()
            .and_then(|b| b.versoes(&id).map_err(|e| e.to_string()))
        {
            Err(_) => return,
            Ok(versoes) => match versoes.last() {
                // Uma nota vazia não precisa de versão-base: a primeira digitação já é a primeira.
                None if texto.is_empty() => return,
                None => Origem::Editor,
                Some(ultima) if ultima.impressao != impressao_do_texto(&texto) => Origem::Externa,
                Some(_) => return,
            },
        };
        let _ = self.gravar(&nota, &texto, origem);
    }

    /// Passa um evento ao agrupador da nota e, se mandar gravar, grava o texto que a tela deu.
    fn evento_da_nota(&mut self, nota: &Path, evento: impl FnOnce(Duration) -> Evento<'static>) {
        let agora = self.agora();
        let Some(n) = self.notas.get_mut(nota) else {
            return;
        };
        let motivo = n.agrupador.evento(evento(agora));
        if motivo.is_some() {
            self.gravar_texto_da_nota(nota, Origem::Editor);
        }
    }

    fn gravar_texto_da_nota(&mut self, nota: &Path, origem: Origem) {
        let Some(n) = self.notas.get_mut(nota) else {
            return;
        };
        let texto = std::mem::take(&mut n.texto);
        let _ = self.gravar(nota, &texto, origem);
        if let Some(n) = self.notas.get_mut(nota) {
            n.texto = texto;
        }
    }

    /// O relógio do agrupador: o que passou da pausa vira versão.
    fn pausas(&mut self) {
        if !self.habilitado {
            return;
        }
        let agora = self.agora();
        let vencidas: Vec<PathBuf> = self
            .notas
            .iter()
            .filter(|(_, n)| n.agrupador.proximo_prazo().is_some_and(|p| p <= agora))
            .map(|(caminho, _)| caminho.clone())
            .collect();
        for nota in vencidas {
            self.evento_da_nota(&nota, |quando| Evento::Pausa { quando });
        }
    }

    fn manutencao(&mut self) {
        if let Some(desde) = self.gravacao_pendente
            && desde.elapsed() >= PERSISTIR_APOS
        {
            self.gravacao_pendente = None;
            if let Some(banco) = &mut self.banco
                && let Err(erro) = banco.persistir()
            {
                self.avisar(format!("Não foi possível guardar o histórico: {erro}"));
            }
        }
        if self.habilitado && self.indisponivel.is_none() && Instant::now() >= self.proxima_poda {
            self.proxima_poda = Instant::now() + PODAR_A_CADA;
            if self.banco().is_ok()
                && let Some(banco) = &mut self.banco
            {
                match banco.podar(SystemTime::now(), &PoliticaRetencao::default()) {
                    Ok(relatorio) if relatorio.versoes_removidas > 0 => {
                        if let Err(erro) = banco.compactar() {
                            self.avisar(format!("Não foi possível compactar o histórico: {erro}"));
                        }
                    }
                    Ok(_) => {}
                    Err(erro) => {
                        self.avisar(format!("Não foi possível podar o histórico: {erro}"));
                    }
                }
            }
        }
    }

    /// Grava o que sobrou e guarda em disco, no fim.
    fn encerrar(&mut self) {
        if self.habilitado {
            let caminhos: Vec<PathBuf> = self.notas.keys().cloned().collect();
            for nota in caminhos {
                self.evento_da_nota(&nota, |_| Evento::Fechou);
            }
        }
        if let Some(banco) = &mut self.banco {
            let _ = banco.persistir();
        }
        self.banco = None;
    }

    /// Um aviso por motivo: o mesmo erro repetido (disco cheio, por exemplo) não enche a tela.
    fn avisar(&mut self, texto: String) {
        if self.ultimo_aviso.as_deref() == Some(texto.as_str()) {
            return;
        }
        self.ultimo_aviso = Some(texto.clone());
        let _ = self.avisos.send(texto);
        (self.acordar)();
    }

    fn gravar(&mut self, nota: &Path, texto: &str, origem: Origem) -> Resultado<bool> {
        self.gravar_em(nota, texto, origem, SystemTime::now())
    }

    fn gravar_em(
        &mut self,
        nota: &Path,
        texto: &str,
        origem: Origem,
        quando: SystemTime,
    ) -> Resultado<bool> {
        let id = id_da_nota(nota);
        let banco = self.banco()?;
        banco.ajustar_quadro_chave(quadro_chave_para(texto.len()));
        let versao = banco
            .gravar(&id, texto, quando, origem)
            .map_err(|e| format!("Não foi possível gravar o histórico: {e}"));
        match versao {
            Ok(versao) => {
                self.gravacao_pendente.get_or_insert_with(Instant::now);
                if versao.is_some() {
                    let _ = self.gravou.send(nota.to_owned());
                    (self.acordar)();
                }
                Ok(versao.is_some())
            }
            Err(erro) => {
                self.avisar(erro.clone());
                Err(erro)
            }
        }
    }

    /// O banco aberto; abre (e migra o histórico antigo) na primeira vez.
    fn banco(&mut self) -> Resultado<&mut Banco> {
        if !self.habilitado {
            return Err("O histórico local está desligado.".into());
        }
        if let Some(motivo) = &self.indisponivel {
            return Err(motivo.clone());
        }
        if self.banco.is_none() {
            match self.abrir() {
                Ok(banco) => self.banco = Some(banco),
                Err(motivo) => {
                    self.indisponivel = Some(motivo.clone());
                    self.avisar(motivo.clone());
                    return Err(motivo);
                }
            }
        }
        self.banco
            .as_mut()
            .ok_or_else(|| "O histórico local não abriu.".into())
    }

    fn abrir(&mut self) -> Resultado<Banco> {
        let arquivo = arquivo_do_cofre(&self.raiz);
        let pasta = arquivo.parent().expect("pasta do histórico");
        fs::create_dir_all(pasta)
            .map_err(|e| format!("Não foi possível criar a pasta do histórico: {e}"))?;
        let mut banco = Banco::abrir(&arquivo).map_err(|e| {
            format!(
                "O histórico das notas não abriu ({e}). Outra janela do Rustidian pode estar usando este cofre; \
                 as notas continuam normais, só sem histórico nesta janela."
            )
        })?;
        match migrar_do_legado(&self.raiz, &mut banco) {
            Ok(_) => {}
            Err(erro) => self.avisar(erro),
        }
        Ok(banco)
    }

    fn apagar_arquivos(&mut self) -> Resultado<()> {
        self.banco = None;
        let arquivo = arquivo_do_cofre(&self.raiz);
        match fs::remove_file(&arquivo) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Não foi possível apagar o histórico local: {e}")),
        }
        // O histórico antigo guardava o mesmo texto: some junto, senão "apagar" não apagaria.
        if let Ok(legado) = Legado::open(&self.raiz) {
            legado.erase()?;
        }
        Ok(())
    }

    fn renomear(&mut self, de: &Path, para: &Path) {
        if de == para {
            return;
        }
        // O estado da tela acompanha (a nota inteira ou tudo o que está na pasta).
        let movidas: Vec<PathBuf> = self
            .notas
            .keys()
            .filter(|k| k.starts_with(de))
            .cloned()
            .collect();
        for antigo in movidas {
            if let Some(n) = self.notas.remove(&antigo) {
                let novo = paths::moved_path(&antigo, de, para, true);
                self.notas.insert(novo, n);
            }
        }
        if !self.habilitado {
            return;
        }
        let ids = match self
            .banco()
            .and_then(|b| b.notas().map_err(|e| e.to_string()))
        {
            Ok(ids) => ids,
            Err(_) => return,
        };
        let prefixo = id_da_nota(de);
        let com_barra = format!("{}/", prefixo.como_str());
        let mut conflitos = Vec::new();
        for id in ids {
            let destino = if id == prefixo {
                id_da_nota(para)
            } else if let Some(resto) = id.como_str().strip_prefix(&com_barra) {
                NotaId::novo(format!("{}/{resto}", id_da_nota(para).como_str()))
            } else {
                continue;
            };
            if let Ok(banco) = self.banco()
                && let Err(erro) = banco.renomear_nota(&id, &destino)
            {
                conflitos.push(format!("{}: {erro}", id.como_str()));
            }
        }
        if !conflitos.is_empty() {
            self.avisar(format!(
                "O histórico de {} não acompanhou a mudança de nome ({}).",
                paths::display_path(de),
                conflitos.join("; ")
            ));
        }
        self.gravacao_pendente.get_or_insert_with(Instant::now);
    }
}

/// Passa o que o histórico antigo (`recovery.redb`, no estado do usuário) guardou para o novo, em
/// ordem, uma vez. O banco antigo não é apagado; a marca `.rustidian/historico-migrado.txt` evita
/// repetir. Uma nota que já tem versões no banco novo é pulada (não duplica numa segunda tentativa).
pub fn migrar_do_legado(raiz: &Path, banco: &mut Banco) -> Resultado<usize> {
    let marca = marca_de_migracao(raiz);
    if marca.exists() {
        return Ok(0);
    }
    let legado = Legado::open(raiz)?;
    let mut total = 0usize;
    let mut notas = 0usize;
    let mut caminhos = legado.files()?;
    caminhos.sort();
    for caminho in caminhos {
        let id = id_da_nota(&caminho);
        if !banco
            .versoes(&id)
            .map_err(|e| format!("Migração do histórico: {e}"))?
            .is_empty()
        {
            continue;
        }
        // O histórico antigo lista da mais nova para a mais antiga.
        let mut versoes = legado.versions(&caminho)?;
        versoes.reverse();
        let mut desta = 0;
        for versao in versoes {
            let texto = legado.content(&caminho, versao.id)?;
            let quando = UNIX_EPOCH + Duration::from_millis(versao.timestamp_ms.max(0) as u64);
            banco.ajustar_quadro_chave(quadro_chave_para(texto.len()));
            if banco
                .gravar(&id, &texto, quando, Origem::Editor)
                .map_err(|e| format!("Migração do histórico: {e}"))?
                .is_some()
            {
                desta += 1;
            }
        }
        total += desta;
        notas += usize::from(desta > 0);
    }
    banco
        .persistir()
        .map_err(|e| format!("Migração do histórico: {e}"))?;
    fs::write(
        &marca,
        format!(
            "Histórico antigo migrado: {total} versões de {notas} notas.\n\
             O banco antigo não foi apagado nesta fase.\n"
        ),
    )
    .map_err(|e| format!("Migração do histórico: {e}"))?;
    Ok(total)
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::sync::{Arc, atomic::AtomicUsize};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn ritmo_rapido() -> Ritmo {
        Ritmo {
            pausa_frase: ms(60),
            pausa_paragrafo: ms(30),
            pausa_geral: ms(150),
            max_caracteres: 300,
        }
    }

    fn abrir(raiz: &Path) -> Historico {
        Historico::iniciar(raiz, true, ritmo_rapido(), || {})
    }

    fn esperar(condicao: impl Fn() -> bool) {
        for _ in 0..400 {
            if condicao() {
                return;
            }
            std::thread::sleep(ms(10));
        }
        panic!("a condição não chegou em 4 s");
    }

    fn versoes(h: &Historico, nota: &str) -> Vec<VersaoInfo> {
        h.leitor().versoes(Path::new(nota)).unwrap()
    }

    #[test]
    fn digitar_uma_frase_e_parar_grava_uma_versao_e_nao_uma_por_letra() {
        let d = tempfile::tempdir().unwrap();
        let h = abrir(d.path());
        let nota = Path::new("a.md");
        h.abriu(nota, "");
        let mut texto = String::new();
        for c in "Olá, mundo. ".chars() {
            texto.push(c);
            h.editou(nota, &texto);
        }
        esperar(|| versoes(&h, "a.md").len() == 1);
        std::thread::sleep(ms(400));
        let v = versoes(&h, "a.md");
        // Nota vazia não tem versão-base: a frase inteira é a primeira e única.
        assert_eq!(v.len(), 1, "uma versão para a frase, não uma por letra");
        assert_eq!(h.leitor().ler(nota, v[0].id).unwrap(), "Olá, mundo. ");
    }

    #[test]
    fn quatrocentos_caracteres_sem_pausa_dao_duas_versoes() {
        let d = tempfile::tempdir().unwrap();
        let h = abrir(d.path());
        let nota = Path::new("b.md");
        h.abriu(nota, "x");
        let mut texto = String::from("x");
        for _ in 0..40 {
            texto.push_str("abcdefghi ");
            h.editou(nota, &texto);
        }
        // 300 caracteres seguidos viram uma versão na hora; o resto, na pausa.
        esperar(|| versoes(&h, "b.md").len() == 3);
        std::thread::sleep(ms(400));
        let v = versoes(&h, "b.md");
        assert_eq!(v.len(), 3, "base + volume + pausa");
        let ultima = h.leitor().ler(nota, v[2].id).unwrap();
        assert_eq!(ultima, texto);
        let do_meio = h.leitor().ler(nota, v[1].id).unwrap();
        assert!(do_meio.len() < texto.len() && do_meio.len() >= 300);
    }

    #[test]
    fn salvar_trocar_de_nota_e_fechar_gravam_o_que_sobrou() {
        let d = tempfile::tempdir().unwrap();
        let mut ritmo = ritmo_rapido();
        ritmo.pausa_geral = Duration::from_secs(60);
        ritmo.pausa_frase = Duration::from_secs(60);
        let h = Historico::iniciar(d.path(), true, ritmo, || {});
        let nota = Path::new("c.md");
        h.abriu(nota, "base");
        h.editou(nota, "base 1");
        h.salvou(nota);
        esperar(|| versoes(&h, "c.md").len() == 2);
        h.editou(nota, "base 2");
        h.saiu(nota);
        esperar(|| versoes(&h, "c.md").len() == 3);
        h.editou(nota, "base 3");
        drop(h); // fechar o app grava e guarda
        let h = abrir(d.path());
        let v = versoes(&h, "c.md");
        assert_eq!(v.len(), 4);
        assert_eq!(h.leitor().ler(nota, v[3].id).unwrap(), "base 3");
    }

    #[test]
    fn reabrir_mantem_o_historico_e_renomear_preserva() {
        let d = tempfile::tempdir().unwrap();
        {
            let h = abrir(d.path());
            let nota = Path::new("pasta/velha.md");
            h.abriu(nota, "um");
            h.editou(nota, "um dois");
            h.salvou(nota);
            esperar(|| versoes(&h, "pasta/velha.md").len() == 2);
            h.renomear(Path::new("pasta/velha.md"), Path::new("outra/nova.md"));
            esperar(|| versoes(&h, "outra/nova.md").len() == 2);
            assert!(versoes(&h, "pasta/velha.md").is_empty());
            // Depois de renomear, a nota nova segue gravando na mesma cadeia.
            h.editou(Path::new("outra/nova.md"), "um dois três");
            h.salvou(Path::new("outra/nova.md"));
            esperar(|| versoes(&h, "outra/nova.md").len() == 3);
        }
        let h = abrir(d.path());
        assert_eq!(versoes(&h, "outra/nova.md").len(), 3);
        assert!(versoes(&h, "pasta/velha.md").is_empty());
    }

    #[test]
    fn renomear_uma_pasta_leva_todas_as_notas() {
        let d = tempfile::tempdir().unwrap();
        let h = abrir(d.path());
        for nome in ["pasta/a.md", "pasta/sub/b.md", "pastinha/c.md"] {
            h.abriu(Path::new(nome), nome);
        }
        esperar(|| {
            versoes(&h, "pastinha/c.md").len() == 1 && versoes(&h, "pasta/sub/b.md").len() == 1
        });
        h.renomear(Path::new("pasta"), Path::new("nova"));
        esperar(|| versoes(&h, "nova/sub/b.md").len() == 1);
        assert_eq!(versoes(&h, "nova/a.md").len(), 1);
        assert!(versoes(&h, "pasta/a.md").is_empty());
        assert_eq!(
            versoes(&h, "pastinha/c.md").len(),
            1,
            "nome parecido não é a pasta"
        );
    }

    #[test]
    fn mudanca_de_fora_vira_versao_com_a_origem() {
        let d = tempfile::tempdir().unwrap();
        let h = abrir(d.path());
        let nota = Path::new("d.md");
        h.abriu(nota, "antes");
        h.gravar(nota, "depois, de fora", Origem::Externa);
        h.gravar(nota, "depois do sync", Origem::Sync);
        esperar(|| versoes(&h, "d.md").len() == 3);
        let origens: Vec<_> = versoes(&h, "d.md").iter().map(|v| v.origem).collect();
        assert_eq!(origens, [Origem::Editor, Origem::Externa, Origem::Sync]);
        // Abrir de novo com um texto diferente da última versão também é "de fora".
        h.fechou(nota);
        h.abriu(nota, "mudou com o app fechado");
        esperar(|| versoes(&h, "d.md").len() == 4);
        assert_eq!(versoes(&h, "d.md")[3].origem, Origem::Externa);
    }

    #[test]
    fn duas_instancias_no_mesmo_cofre_a_segunda_fica_sem_historico_e_avisa_uma_vez() {
        let d = tempfile::tempdir().unwrap();
        let primeira = abrir(d.path());
        primeira.abriu(Path::new("e.md"), "texto");
        esperar(|| versoes(&primeira, "e.md").len() == 1);
        let avisos = Arc::new(AtomicUsize::new(0));
        let conta = avisos.clone();
        let segunda = Historico::iniciar(d.path(), true, ritmo_rapido(), move || {
            conta.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        segunda.abriu(Path::new("e.md"), "outro");
        assert!(segunda.leitor().versoes(Path::new("e.md")).is_err());
        segunda.editou(Path::new("e.md"), "outro mais");
        segunda.salvou(Path::new("e.md"));
        std::thread::sleep(ms(100));
        let todos: Vec<String> = segunda.avisos.try_iter().collect();
        assert_eq!(todos.len(), 1, "um aviso só: {todos:?}");
        // A primeira segue funcionando.
        assert_eq!(versoes(&primeira, "e.md").len(), 1);
    }

    #[test]
    fn desligar_apaga_o_arquivo_e_nao_grava() {
        let d = tempfile::tempdir().unwrap();
        let mut h = abrir(d.path());
        h.abriu(Path::new("f.md"), "texto");
        esperar(|| versoes(&h, "f.md").len() == 1);
        assert!(arquivo_do_cofre(d.path()).exists());
        h.configurar(false);
        esperar(|| !arquivo_do_cofre(d.path()).exists());
        h.gravar(Path::new("f.md"), "mais", Origem::Externa);
        std::thread::sleep(ms(100));
        assert!(!arquivo_do_cofre(d.path()).exists());
    }

    #[test]
    fn abrir_desligado_apaga_o_arquivo_que_ficou() {
        let d = tempfile::tempdir().unwrap();
        {
            let h = abrir(d.path());
            h.abriu(Path::new("i.md"), "texto");
            esperar(|| versoes(&h, "i.md").len() == 1);
        }
        assert!(arquivo_do_cofre(d.path()).exists());
        let h = Historico::iniciar(d.path(), false, ritmo_rapido(), || {});
        esperar(|| !arquivo_do_cofre(d.path()).exists());
        drop(h);
    }

    #[test]
    fn limpar_apaga_tudo_e_volta_a_gravar() {
        let d = tempfile::tempdir().unwrap();
        let h = abrir(d.path());
        h.abriu(Path::new("g.md"), "segredo");
        esperar(|| versoes(&h, "g.md").len() == 1);
        h.limpar().unwrap().recv().unwrap().unwrap();
        assert!(h.leitor().notas().unwrap().is_empty());
        h.gravar(Path::new("g.md"), "novo", Origem::Externa);
        esperar(|| versoes(&h, "g.md").len() == 1);
    }

    #[test]
    fn apagar_texto_some_das_versoes() {
        let d = tempfile::tempdir().unwrap();
        let h = abrir(d.path());
        let nota = Path::new("h.md");
        h.abriu(nota, "a senha é HUNTER2 mesmo");
        h.gravar(nota, "a senha é HUNTER2 mesmo, e mais", Origem::Externa);
        esperar(|| versoes(&h, "h.md").len() == 2);
        let n = h
            .apagar_texto(nota, "HUNTER2")
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        assert_eq!(n, 2);
        for v in versoes(&h, "h.md") {
            assert!(!h.leitor().ler(nota, v.id).unwrap().contains("HUNTER2"));
        }
    }

    #[test]
    fn migra_o_historico_antigo_em_ordem_e_uma_vez_sem_apagar_o_banco_antigo() {
        use crate::recovery::{Config, History};
        let base = tempfile::tempdir().unwrap();
        let raiz = base.path().join("cofre");
        fs::create_dir(&raiz).unwrap();
        let antigo = History::open(&raiz).unwrap();
        let config = Config::default();
        let nota = Path::new("Projetos/plano.md");
        // Recentes: a poda de retenção não pode ter o que tirar na primeira abertura.
        let agora = chrono::Local::now().timestamp_millis() - 3_600_000;
        for (i, texto) in ["um", "um dois", "um dois três"].iter().enumerate() {
            antigo
                .record(nota, texto, agora + i as i64 * 60_000, &config, true)
                .unwrap();
        }
        antigo
            .record(
                Path::new("solta.md"),
                "só uma",
                agora + 500_000,
                &config,
                true,
            )
            .unwrap();
        let h = abrir(&raiz);
        let v = versoes(&h, "Projetos/plano.md");
        assert_eq!(v.len(), 3);
        let textos: Vec<String> = v
            .iter()
            .map(|v| h.leitor().ler(nota, v.id).unwrap())
            .collect();
        assert_eq!(textos, ["um", "um dois", "um dois três"]);
        assert!(v[0].quando < v[2].quando);
        assert_eq!(versoes(&h, "solta.md").len(), 1);
        assert!(marca_de_migracao(&raiz).exists());
        assert_eq!(
            antigo.versions(nota).unwrap().len(),
            3,
            "o banco antigo continua"
        );
        drop(h);
        // Uma segunda abertura não repete.
        let h = abrir(&raiz);
        assert_eq!(versoes(&h, "Projetos/plano.md").len(), 3);
    }

    #[test]
    fn quadro_chave_menor_para_notas_grandes() {
        assert_eq!(quadro_chave_para(5_000), 128);
        assert_eq!(quadro_chave_para(300_000), 48);
        assert_eq!(quadro_chave_para(2_000_000), 32);
    }
}
