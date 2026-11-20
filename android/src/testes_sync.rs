//! Integração da sincronização (R7b-3), sem Dioxus: o `nota-servidor` de verdade numa porta
//! local (porta 0) e dois ou três "celulares" (cofre e estado em pastas temporárias) usando a
//! mesma lógica do app (`Sincronia`, `Cofre`, `Rascunho`).
//!
//! Cobre: (1) convergência; (2) mudança nos dois lados gera as duas cópias; (3) apagar vai para
//! a lixeira nos dois; (4) renomear durante um ciclo não perde nada e espera o portão; (5) frase
//! errada dá erro claro e não grava lixo; mais o editor × sync, o servidor fora do ar e reabrir
//! o app (a frase é pedida de novo).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nota_cripto::ChaveCofre;
use nota_servidor::{ConfigServidor, ServidorLocal, tokens};
use nota_sync::cofre::listar_vivos;
use nota_sync::{Config, Opcoes, RemotoHttp, Sincronizador};

use crate::logica::agenda::Tempos;
use crate::logica::ajustes::Arquivos;
use crate::logica::cofre::Cofre;
use crate::logica::conexao::{ErroConexao, Resultado};
use crate::logica::portao::Gated;
use crate::logica::rascunho::{Estado as EstadoRascunho, Rascunho};
use crate::logica::sincronia::{Entrada, EstadoSync, Fase, Sincronia};

const FRASE: &str = "uma frase de teste bem longa";
const LIMITE: Duration = Duration::from_secs(60);

struct Servidor {
    dados: tempfile::TempDir,
    servidor: Option<ServidorLocal>,
    /// O token de cada aparelho (o servidor não deixa criar dois para o mesmo nome).
    tokens: std::cell::RefCell<BTreeMap<String, String>>,
}

impl Servidor {
    fn novo() -> Self {
        let dados = tempfile::tempdir().unwrap();
        let servidor = ServidorLocal::iniciar(ConfigServidor {
            dados: dados.path().to_path_buf(),
            escutar: "127.0.0.1:0".parse().unwrap(),
            permitir_todas_interfaces: false,
        })
        .unwrap();
        Self {
            dados,
            servidor: Some(servidor),
            tokens: std::cell::RefCell::new(BTreeMap::new()),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.servidor.as_ref().unwrap().endereco())
    }

    fn token(&self, aparelho: &str) -> String {
        self.tokens
            .borrow_mut()
            .entry(aparelho.to_string())
            .or_insert_with(|| tokens::novo(self.dados.path(), aparelho).unwrap())
            .clone()
    }

    fn config(&self, aparelho: &str) -> Config {
        Config {
            servidor: self.url(),
            token: self.token(aparelho),
            aparelho: aparelho.into(),
        }
    }

    fn derrubar(&mut self) {
        self.servidor = None;
    }
}

struct Celular {
    _t: tempfile::TempDir,
    cofre: Cofre,
    sync: Sincronia,
    arquivos: Arquivos,
    nome: String,
}

impl Celular {
    fn novo(nome: &str) -> Self {
        let t = tempfile::tempdir().unwrap();
        let cofre = Cofre::abrir(t.path().join("Cofre")).unwrap();
        let arquivos = Arquivos::em(t.path().join("sync"));
        let sync = Sincronia::nova(cofre.clone(), arquivos.clone(), Tempos::default());
        Self {
            _t: t,
            cofre,
            sync,
            arquivos,
            nome: nome.into(),
        }
    }

    fn entrada(&self, srv: &Servidor, frase: &str) -> Entrada {
        Entrada {
            servidor: srv.url(),
            aparelho: self.nome.clone(),
            token: srv.token(&self.nome),
            frase: frase.into(),
        }
    }

    fn conectar(&self, srv: &Servidor, frase: &str) -> Result<Resultado, ErroConexao> {
        self.sync.conectar(self.entrada(srv, frase))
    }

    /// Espera o ciclo em andamento (se houver), pede um novo e espera ele terminar.
    fn sincronizar(&self) -> EstadoSync {
        let atual = self
            .sync
            .esperar_ciclo(0, LIMITE)
            .expect("o primeiro ciclo terminou");
        let n = atual.ciclos;
        self.sync.agora();
        self.sync
            .esperar_ciclo(n, LIMITE)
            .expect("o ciclo pedido terminou")
    }

    fn notas(&self) -> BTreeMap<String, String> {
        listar_vivos(self.cofre.raiz())
            .unwrap()
            .into_iter()
            .map(|(k, v)| (k, String::from_utf8(v).unwrap()))
            .collect()
    }

    /// O que está na lixeira do cofre: caminho (sem `.trash/`) → texto.
    fn lixeira(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        coletar(&self.cofre.raiz().join(".trash"), "", &mut m);
        m
    }
}

fn coletar(dir: &Path, rel: &str, saida: &mut BTreeMap<String, String>) {
    let Ok(es) = fs::read_dir(dir) else {
        return;
    };
    for e in es.flatten() {
        let nome = e.file_name().to_string_lossy().into_owned();
        let caminho = if rel.is_empty() {
            nome.clone()
        } else {
            format!("{rel}/{nome}")
        };
        if e.file_type().unwrap().is_dir() {
            coletar(&e.path(), &caminho, saida);
        } else {
            saida.insert(caminho, fs::read_to_string(e.path()).unwrap());
        }
    }
}

fn dois(srv: &Servidor) -> (Celular, Celular) {
    let a = Celular::novo("pc");
    let b = Celular::novo("celular");
    assert_eq!(a.conectar(srv, FRASE).unwrap(), Resultado::Criou);
    assert_eq!(b.conectar(srv, FRASE).unwrap(), Resultado::Entrou);
    (a, b)
}

#[test]
fn os_dois_convergem() {
    let srv = Servidor::novo();
    let (a, b) = dois(&srv);

    a.cofre
        .salvar_nota("Ideias.md", "# Ideias\n\num\n")
        .unwrap();
    a.cofre.criar_pasta("", "Receitas").unwrap();
    a.cofre
        .salvar_nota("Receitas/Bolo.md", "ovos e farinha\n")
        .unwrap();
    let ea = a.sincronizar();
    assert_eq!(ea.fase, Fase::Sincronizado, "{:?}", ea.erro);
    let eb = b.sincronizar();
    assert_eq!(eb.fase, Fase::Sincronizado, "{:?}", eb.erro);
    assert_eq!(a.notas(), b.notas());
    assert_eq!(b.notas().len(), 2);
    assert!(eb.ultima.is_some());

    // O celular edita e cria; o PC recebe.
    b.cofre
        .salvar_nota("Ideias.md", "# Ideias\n\num\ndois\n")
        .unwrap();
    b.cofre.salvar_nota("Nova.md", "do celular\n").unwrap();
    b.sincronizar();
    a.sincronizar();
    assert_eq!(a.notas(), b.notas());
    assert_eq!(a.notas()["Ideias.md"], "# Ideias\n\num\ndois\n");
    assert_eq!(a.notas()["Nova.md"], "do celular\n");
    // Convergiram sem inventar conflito.
    assert!(a.sync.estado().conflitos.is_empty());
    assert!(b.sync.estado().conflitos.is_empty());
}

#[test]
fn mudanca_nos_dois_lados_guarda_as_duas_copias() {
    let srv = Servidor::novo();
    let (a, b) = dois(&srv);
    a.cofre.salvar_nota("n.md", "base\n").unwrap();
    a.sincronizar();
    b.sincronizar();

    // Cada um edita sem ter visto o outro.
    a.cofre.salvar_nota("n.md", "versão do PC\n").unwrap();
    b.cofre.salvar_nota("n.md", "versão do celular\n").unwrap();
    a.sincronizar();
    b.sincronizar();
    // O segundo ciclo do celular sobe a cópia de conflito que o primeiro criou.
    let eb = b.sincronizar();

    // No celular: a versão do PC é a nota; a do celular virou cópia de conflito (nada some).
    assert_eq!(b.notas()["n.md"], "versão do PC\n");
    assert_eq!(eb.conflitos.len(), 1, "{:?}", eb.conflitos);
    assert!(
        eb.conflitos[0].starts_with("n (conflito celular "),
        "{:?}",
        eb.conflitos
    );
    assert_eq!(b.notas()[&eb.conflitos[0]], "versão do celular\n");
    assert_eq!(eb.rotulo().texto, "1 conflito");

    // O PC também fica com as duas.
    let ea = a.sincronizar();
    assert_eq!(a.notas(), b.notas());
    assert_eq!(ea.conflitos, eb.conflitos);
}

#[test]
fn nota_apagada_vai_para_a_lixeira_nos_dois_lados() {
    let srv = Servidor::novo();
    let (a, b) = dois(&srv);
    a.cofre
        .salvar_nota("Velha.md", "conteúdo que não pode sumir\n")
        .unwrap();
    a.cofre.salvar_nota("Fica.md", "fica\n").unwrap();
    a.sincronizar();
    b.sincronizar();
    assert_eq!(b.notas().len(), 2);

    // Apagar no celular (pelo app: lixeira do cofre) e sincronizar nos dois.
    b.cofre.para_a_lixeira("Velha.md").unwrap();
    b.sincronizar();
    a.sincronizar();

    for ap in [&a, &b] {
        assert_eq!(
            ap.notas().keys().collect::<Vec<_>>(),
            ["Fica.md"],
            "{}",
            ap.nome
        );
        let lixo = ap.lixeira();
        assert_eq!(lixo.len(), 1, "{}: {lixo:?}", ap.nome);
        let (nome, texto) = lixo.iter().next().unwrap();
        assert!(nome.starts_with("Velha "), "{nome}");
        assert_eq!(texto, "conteúdo que não pode sumir\n");
    }
}

#[test]
fn renomear_espera_o_ciclo_gravando_e_nao_perde_nada() {
    let srv = Servidor::novo();
    let chave = ChaveCofre::nova();
    let copia_da_chave = || ChaveCofre::de_bytes(&chave.bytes()[..]).unwrap();

    // O PC (cofre, portão e sincronizador) montado à mão, para segurar o ciclo no meio.
    let t = tempfile::tempdir().unwrap();
    let cofre = Cofre::abrir(t.path().join("Cofre")).unwrap();
    cofre.salvar_nota("a.md", "texto da nota a\n").unwrap();
    cofre.salvar_nota("b.md", "texto da nota b\n").unwrap();
    let remoto = RemotoHttp::novo(&srv.config("pc")).unwrap();
    let mut sinc = Sincronizador::novo(
        Opcoes {
            cofre: cofre.raiz().to_path_buf(),
            estado: t.path().join("estado.redb"),
            aparelho: "pc".into(),
        },
        copia_da_chave(),
        Gated::novo(remoto, Arc::clone(&cofre.portao().0)),
    )
    .unwrap();

    let (no_efeito_tx, no_efeito_rx) = mpsc::channel::<()>();
    let (seguir_tx, seguir_rx) = mpsc::channel::<()>();
    let seguir_rx = std::sync::Mutex::new(seguir_rx);
    sinc.definir_gancho_efeito(Some(Box::new(move |n, _| {
        if n == 0 {
            no_efeito_tx.send(()).unwrap();
            // Fica aqui, com o portão preso (trecho de disco do ciclo), até o teste mandar seguir.
            let _ = seguir_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30));
        }
    })));

    let ciclo = std::thread::spawn(move || {
        sinc.remoto().entrar();
        let r = sinc.ciclo();
        sinc.remoto().sair();
        (sinc, r)
    });
    no_efeito_rx
        .recv_timeout(LIMITE)
        .expect("o ciclo chegou ao primeiro efeito");
    assert!(
        cofre.portao().0.esta_preso(),
        "o ciclo segura o portão no disco"
    );

    // Renomear agora tem de esperar.
    let renomeou = Arc::new(AtomicBool::new(false));
    let renomear = {
        let cofre = cofre.clone();
        let renomeou = Arc::clone(&renomeou);
        std::thread::spawn(move || {
            let r = cofre.renomear_nota("a.md", "Renomeada");
            renomeou.store(true, Ordering::SeqCst);
            r
        })
    };
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !renomeou.load(Ordering::SeqCst),
        "renomear não pode rodar no meio do trecho de disco do ciclo"
    );
    seguir_tx.send(()).unwrap();
    let (mut sinc, r) = ciclo.join().unwrap();
    r.expect("o ciclo terminou bem");
    let novo = renomear.join().unwrap().unwrap();
    assert_eq!(novo, "Renomeada.md");
    assert!(renomeou.load(Ordering::SeqCst));
    assert!(!cofre.portao().0.esta_preso());

    // Outro ciclo (agora vê o nome novo) e um segundo aparelho que só recebe.
    sinc.remoto().entrar();
    sinc.ciclo().expect("segundo ciclo");
    sinc.remoto().sair();

    let t2 = tempfile::tempdir().unwrap();
    let cofre2 = Cofre::abrir(t2.path().join("Cofre")).unwrap();
    let mut sinc2 = Sincronizador::novo(
        Opcoes {
            cofre: cofre2.raiz().to_path_buf(),
            estado: t2.path().join("estado.redb"),
            aparelho: "celular".into(),
        },
        copia_da_chave(),
        RemotoHttp::novo(&srv.config("celular")).unwrap(),
    )
    .unwrap();
    sinc2.ciclo().expect("ciclo do segundo aparelho");

    let vivos = listar_vivos(cofre2.raiz()).unwrap();
    let texto = |k: &str| String::from_utf8(vivos.get(k).cloned().unwrap_or_default()).unwrap();
    assert_eq!(
        texto("Renomeada.md"),
        "texto da nota a\n",
        "{:?}",
        vivos.keys()
    );
    assert_eq!(texto("b.md"), "texto da nota b\n");
    // O nome antigo não ficou vivo no PC nem no celular.
    assert!(!cofre.raiz().join("a.md").exists());
    assert!(!vivos.contains_key("a.md"));
}

#[test]
fn frase_errada_da_erro_claro_e_nao_grava_nada() {
    let srv = Servidor::novo();
    let a = Celular::novo("pc");
    a.conectar(&srv, FRASE).unwrap();
    a.cofre.salvar_nota("n.md", "x\n").unwrap();
    a.sincronizar();

    // Aparelho novo com a frase errada: erro claro, nada gravado, nada ligado.
    let c = Celular::novo("celular");
    let e = c
        .conectar(&srv, "outra frase qualquer, bem longa")
        .unwrap_err();
    assert_eq!(e, ErroConexao::FraseErrada);
    assert!(e.to_string().starts_with("Frase-senha incorreta"), "{e}");
    assert!(
        !c.arquivos.config().exists(),
        "não pode gravar configuração"
    );
    assert!(
        !c.arquivos.banco().exists(),
        "não pode criar o estado interno"
    );
    assert!(!c.arquivos.pasta().exists() || fs::read_dir(c.arquivos.pasta()).unwrap().count() == 0);
    assert!(c.notas().is_empty());
    let es = c.sync.estado();
    assert_eq!(es.fase, Fase::SemConfig);
    assert!(es.erro.unwrap().starts_with("Frase-senha incorreta"));
    assert!(!c.sync.ajustes().conectado);

    // Token errado: outro erro claro.
    let mut entrada = c.entrada(&srv, FRASE);
    entrada.token = "nst_que_nao_existe".into();
    let e = c.sync.conectar(entrada).unwrap_err();
    assert_eq!(e, ErroConexao::NaoAutorizado);
    assert!(!c.arquivos.config().exists());

    // Endereço inutilizável: nem tenta a rede.
    let mut entrada = c.entrada(&srv, FRASE);
    entrada.servidor = "https://100.1.2.3:8443".into();
    assert!(matches!(
        c.sync.conectar(entrada),
        Err(ErroConexao::Ajustes(_))
    ));

    // Com a frase certa funciona (o estado não ficou sujo pelas falhas).
    assert_eq!(c.conectar(&srv, FRASE).unwrap(), Resultado::Entrou);
    c.sincronizar();
    assert_eq!(c.notas()["n.md"], "x\n");
}

#[test]
fn reabrir_o_app_pede_a_frase_de_novo_e_errar_nao_desconecta() {
    let srv = Servidor::novo();
    let a = Celular::novo("pc");
    a.conectar(&srv, FRASE).unwrap();
    a.cofre.salvar_nota("n.md", "x\n").unwrap();
    a.sincronizar();
    a.sync.parar();

    // "Reabrir o app": nova Sincronia sobre as mesmas pastas.
    let sync = Sincronia::nova(a.cofre.clone(), a.arquivos.clone(), Tempos::default());
    assert_eq!(sync.estado().fase, Fase::PedeFrase);
    assert!(sync.ajustes().conectado);
    // O arquivo de configuração não tem a frase nem a chave.
    let json = fs::read_to_string(a.arquivos.config()).unwrap();
    assert!(
        !json.contains(FRASE) && !json.to_lowercase().contains("frase"),
        "{json}"
    );

    // Frase errada: erro claro e continua pedindo a frase (a conexão salva não some).
    let e = sync.entrar("frase errada mas longa").unwrap_err();
    assert_eq!(e, ErroConexao::FraseErrada);
    assert_eq!(sync.estado().fase, Fase::PedeFrase);
    assert!(sync.ajustes().conectado);

    // Frase certa: volta a sincronizar, sem recomeçar do zero.
    assert_eq!(sync.entrar(FRASE).unwrap(), Resultado::Entrou);
    let est = sync
        .esperar_ciclo(0, LIMITE)
        .expect("ciclo depois de entrar");
    assert_eq!(est.fase, Fase::Sincronizado, "{:?}", est.erro);
    assert!(est.conflitos.is_empty());
    assert_eq!(a.notas()["n.md"], "x\n");
    sync.parar();
}

#[test]
fn editor_nao_pisa_na_versao_que_o_sync_trouxe() {
    let srv = Servidor::novo();
    let (a, b) = dois(&srv);
    a.cofre.salvar_nota("n.md", "base\n").unwrap();
    a.sincronizar();
    b.sincronizar();

    // No celular o dono abre a nota e começa a digitar (ainda sem gravar).
    let mut rascunho = Rascunho::default();
    rascunho.abrir("n.md", b.cofre.ler_nota("n.md").unwrap());
    rascunho.digitou("base\nlinha do celular\n".into(), Instant::now());

    // Enquanto isso o PC muda a nota e o celular sincroniza.
    a.cofre.salvar_nota("n.md", "base\nlinha do PC\n").unwrap();
    a.sincronizar();
    b.sincronizar();
    assert_eq!(b.notas()["n.md"], "base\nlinha do PC\n");

    // A gravação do editor não pisa na versão nova: vai para a cópia de conflito.
    rascunho.salvar(&b.cofre);
    assert_eq!(rascunho.estado(), EstadoRascunho::Salvo);
    assert_eq!(b.notas()["n.md"], "base\nlinha do PC\n");
    let copia = rascunho.desvio().expect("desviou").to_string();
    assert_eq!(b.notas()[&copia], "base\nlinha do celular\n");

    // E a cópia sobe: o PC fica com as duas.
    let eb = b.sincronizar();
    assert_eq!(eb.conflitos, std::slice::from_ref(&copia));
    a.sincronizar();
    assert_eq!(a.notas()[&copia], "base\nlinha do celular\n");
    assert_eq!(a.notas()["n.md"], "base\nlinha do PC\n");
}

#[test]
fn servidor_fora_do_ar_nao_trava_nem_perde_nota_e_diz_o_que_fazer() {
    let mut srv = Servidor::novo();
    let a = Celular::novo("pc");
    a.conectar(&srv, FRASE).unwrap();
    a.cofre.salvar_nota("n.md", "x\n").unwrap();
    a.sincronizar();
    srv.derrubar();

    a.cofre.salvar_nota("n.md", "x\ny\n").unwrap();
    let e = a.sincronizar();
    assert_eq!(e.fase, Fase::SemConexao);
    assert!(e.erro.as_deref().unwrap().contains("Tailscale"), "{e:?}");
    assert!(e.motivo_rede.is_some());
    assert_eq!(e.rotulo().texto, "Sem conexão");
    // As notas continuam à mão, editáveis.
    assert_eq!(a.notas()["n.md"], "x\ny\n");
    a.cofre.salvar_nota("outra.md", "z\n").unwrap();
    assert_eq!(a.notas().len(), 2);
}

#[test]
fn desconectar_apaga_o_token_e_o_estado_mas_nao_as_notas() {
    let srv = Servidor::novo();
    let a = Celular::novo("pc");
    a.conectar(&srv, FRASE).unwrap();
    a.cofre.salvar_nota("n.md", "x\n").unwrap();
    a.sincronizar();
    assert!(a.arquivos.banco().exists());
    assert!(!a.sync.ajustes().token.is_empty());

    a.sync.desconectar().unwrap();
    let aj = a.sync.ajustes();
    assert!(!aj.conectado);
    assert!(aj.token.is_empty());
    assert!(!aj.servidor.is_empty(), "o endereço fica para reconectar");
    assert!(!a.arquivos.banco().exists());
    assert_eq!(a.sync.estado().fase, Fase::SemConfig);
    assert_eq!(a.notas()["n.md"], "x\n");
}

#[test]
fn um_ciclo_por_vez_pedidos_repetidos_viram_um() {
    let srv = Servidor::novo();
    let a = Celular::novo("pc");
    a.conectar(&srv, FRASE).unwrap();
    let antes = a.sincronizar().ciclos;
    // Vários pedidos seguidos de "agora" enquanto roda: não acumulam um ciclo para cada.
    for _ in 0..5 {
        a.sync.agora();
    }
    let est = a.sync.esperar_ciclo(antes, LIMITE).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let depois = a.sync.estado().ciclos;
    assert!(depois <= est.ciclos + 1, "{depois} vs {}", est.ciclos);
    assert!(
        depois <= antes + 2,
        "{depois} ciclos para 5 pedidos (antes: {antes})"
    );
}
