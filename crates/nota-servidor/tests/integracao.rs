//! O servidor de verdade numa porta local, o cliente `RemotoHttp` e dois cofres.

use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;

use nota_cripto::ChaveCofre;
use nota_servidor::{ConfigServidor, ServidorLocal, tokens};
use nota_sync::protocolo::{BLOB_MAXIMO, sha256_hex};
use nota_sync::{Config, ErroSync, Opcoes, RemotoHttp, Sincronizador};

struct Ambiente {
    dados: tempfile::TempDir,
    servidor: Option<ServidorLocal>,
    token: String,
}

impl Ambiente {
    fn novo() -> Self {
        let dados = tempfile::tempdir().unwrap();
        let token = tokens::novo(dados.path(), "pc").unwrap();
        let servidor = ServidorLocal::iniciar(ConfigServidor {
            dados: dados.path().to_path_buf(),
            escutar: "127.0.0.1:0".parse().unwrap(),
            permitir_todas_interfaces: false,
        })
        .unwrap();
        Self {
            dados,
            servidor: Some(servidor),
            token,
        }
    }

    fn endereco(&self) -> SocketAddr {
        self.servidor.as_ref().unwrap().endereco()
    }

    fn url(&self, caminho: &str) -> String {
        format!("http://{}{caminho}", self.endereco())
    }

    fn config(&self, aparelho: &str) -> Config {
        Config {
            servidor: format!("http://{}", self.endereco()),
            token: self.token.clone(),
            aparelho: aparelho.into(),
        }
    }

    fn parar(&mut self) {
        self.servidor = None;
    }
}

fn agente() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}

fn status(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> u16 {
    r.expect("o servidor respondeu").status().as_u16()
}

#[test]
fn sem_token_e_token_errado_dao_401_em_todas_as_rotas() {
    let a = Ambiente::novo();
    let ag = agente();
    let hash = sha256_hex(b"x");
    let id = sha256_hex(b"id");
    // (descrição, valor do cabeçalho Authorization; `None` = sem cabeçalho)
    let tentativas: Vec<(&str, Option<String>)> = vec![
        ("sem cabeçalho", None),
        ("token errado", Some("Bearer nst_errado".into())),
        ("esquema errado", Some(format!("Basic {}", a.token))),
        ("bearer vazio", Some("Bearer ".into())),
        (
            "token certo com um byte a mais",
            Some(format!("Bearer {}x", a.token)),
        ),
    ];
    for (nome, cab) in tentativas {
        let com = |mut req: ureq::RequestBuilder<ureq::typestate::WithoutBody>| {
            if let Some(v) = &cab {
                req = req.header("Authorization", v);
            }
            req
        };
        let com_corpo = |mut req: ureq::RequestBuilder<ureq::typestate::WithBody>| {
            if let Some(v) = &cab {
                req = req.header("Authorization", v);
            }
            req
        };
        assert_eq!(
            status(com(ag.get(a.url("/v1/mudancas?desde=0"))).call()),
            401,
            "{nome}"
        );
        assert_eq!(
            status(com(ag.get(a.url(&format!("/v1/blob/{hash}")))).call()),
            401,
            "{nome}"
        );
        assert_eq!(
            status(com_corpo(ag.put(a.url("/v1/blob"))).send(&b"abc"[..])),
            401,
            "{nome}"
        );
        assert_eq!(
            status(
                com_corpo(ag.post(a.url(&format!("/v1/item/{id}"))))
                    .content_type("application/json")
                    .send(&b"{}"[..])
            ),
            401,
            "{nome}"
        );
    }
    // e com o token certo funciona
    let r = ag
        .get(a.url("/v1/mudancas?desde=0"))
        .header("Authorization", format!("Bearer {}", a.token))
        .call();
    assert_eq!(status(r), 200);
}

#[test]
fn rotas_e_validacoes() {
    let a = Ambiente::novo();
    let ag = agente();
    let aut = format!("Bearer {}", a.token);

    // blob com hash que não bate: 400, e nada é guardado
    let r = ag
        .put(a.url("/v1/blob"))
        .header("Authorization", &aut)
        .header("X-Hash-Sha256", sha256_hex(b"outra coisa"))
        .send(&b"conteudo cifrado qualquer"[..]);
    assert_eq!(status(r), 400);
    assert_eq!(
        fs::read_dir(a.dados.path().join("blobs")).unwrap().count(),
        0
    );

    // hash malformado no cabeçalho: 400
    let r = ag
        .put(a.url("/v1/blob"))
        .header("Authorization", &aut)
        .header("X-Hash-Sha256", "zzz")
        .send(&b"abc"[..]);
    assert_eq!(status(r), 400);

    // corpo vazio: 400
    let r = ag
        .put(a.url("/v1/blob"))
        .header("Authorization", &aut)
        .send(&b""[..]);
    assert_eq!(status(r), 400);

    // blob certo: 200 com o hash
    let corpo = b"blob opaco".to_vec();
    let hash = sha256_hex(&corpo);
    let mut r = ag
        .put(a.url("/v1/blob"))
        .header("Authorization", &aut)
        .header("X-Hash-Sha256", &hash)
        .send(&corpo[..])
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert!(r.body_mut().read_to_string().unwrap().contains(&hash));

    // baixar: os mesmos bytes; hash estranho e hash desconhecido
    let mut r = ag
        .get(a.url(&format!("/v1/blob/{hash}")))
        .header("Authorization", &aut)
        .call()
        .unwrap();
    assert_eq!(r.body_mut().read_to_vec().unwrap(), corpo);
    let r = ag
        .get(a.url("/v1/blob/..%2F..%2Ftokens"))
        .header("Authorization", &aut)
        .call();
    assert_eq!(status(r), 400);
    let r = ag
        .get(a.url(&format!("/v1/blob/{}", sha256_hex(b"nao existe"))))
        .header("Authorization", &aut)
        .call();
    assert_eq!(status(r), 404);

    // publicar: blob desconhecido e id estranho = 400
    let id = sha256_hex(b"item");
    let pedido = |base: u64, hash: &str| {
        format!(r#"{{"versao_base":{base},"hash_blob":"{hash}","apagado":false}}"#)
    };
    let post = |id: &str, corpo: String| {
        ag.post(a.url(&format!("/v1/item/{id}")))
            .header("Authorization", &aut)
            .content_type("application/json")
            .send(corpo.as_bytes())
    };
    assert_eq!(status(post(&id, pedido(0, &sha256_hex(b"fantasma")))), 400);
    assert_eq!(status(post("nao-e-hex", pedido(0, &hash))), 400);
    // publicar: 200, e a base velha dá 409 com o estado atual
    let mut r = post(&id, pedido(0, &hash)).unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert!(
        r.body_mut()
            .read_to_string()
            .unwrap()
            .contains("\"versao\":1")
    );
    let mut r = post(&id, pedido(0, &hash)).unwrap();
    assert_eq!(r.status().as_u16(), 409);
    let corpo409 = r.body_mut().read_to_string().unwrap();
    assert!(corpo409.contains("\"versao_atual\":1"), "{corpo409}");
    assert!(corpo409.contains(&hash), "{corpo409}");

    // mudanças trazem o item
    let mut r = ag
        .get(a.url("/v1/mudancas?desde=0"))
        .header("Authorization", &aut)
        .call()
        .unwrap();
    let t = r.body_mut().read_to_string().unwrap();
    assert!(t.contains(&id) && t.contains("\"mais\":false"), "{t}");

    // blob acima do limite: 413 (ou o servidor fecha a conexão antes de ler tudo)
    let enorme = vec![0u8; BLOB_MAXIMO + 1];
    let resposta = ag
        .put(a.url("/v1/blob"))
        .header("Authorization", &aut)
        .send(&enorme[..]);
    if let Ok(r) = resposta {
        assert_eq!(r.status().as_u16(), 413);
    }
    assert!(
        fs::read_dir(a.dados.path().join("blobs"))
            .unwrap()
            .all(|e| e.unwrap().metadata().unwrap().len() < BLOB_MAXIMO as u64)
    );
}

fn ler_tudo(dir: &Path, saida: &mut Vec<u8>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            ler_tudo(&p, saida);
        } else {
            saida.extend(fs::read(&p).unwrap_or_default());
        }
    }
}

fn sinc(
    a: &Ambiente,
    nome: &str,
    chave: &ChaveCofre,
) -> (
    Sincronizador<RemotoHttp>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let cofre = tempfile::tempdir().unwrap();
    let estado = tempfile::tempdir().unwrap();
    let remoto = RemotoHttp::novo(&a.config(nome)).unwrap();
    let s = Sincronizador::novo(
        Opcoes {
            cofre: cofre.path().to_path_buf(),
            estado: estado.path().join("sync.redb"),
            aparelho: nome.into(),
        },
        ChaveCofre::de_bytes(&chave.bytes()[..]).unwrap(),
        remoto,
    )
    .unwrap();
    (s, cofre, estado)
}

#[test]
fn dois_cofres_convergem_pelo_servidor_real() {
    let a = Ambiente::novo();
    let chave = ChaveCofre::nova();
    let (mut pc, cofre_pc, _e1) = sinc(&a, "pc", &chave);
    let (mut cel, cofre_cel, _e2) = sinc(&a, "celular", &chave);

    fs::create_dir_all(cofre_pc.path().join("pasta")).unwrap();
    fs::write(cofre_pc.path().join("a.md"), "texto muito secreto").unwrap();
    fs::write(cofre_pc.path().join("pasta/b.md"), "outra nota").unwrap();
    pc.ciclo().unwrap();
    cel.ciclo().unwrap();
    assert_eq!(
        fs::read_to_string(cofre_cel.path().join("a.md")).unwrap(),
        "texto muito secreto"
    );
    assert_eq!(
        fs::read_to_string(cofre_cel.path().join("pasta/b.md")).unwrap(),
        "outra nota"
    );

    // edições nos dois lados: conflito, nada se perde
    fs::write(cofre_pc.path().join("a.md"), "edicao do pc").unwrap();
    fs::write(cofre_cel.path().join("a.md"), "edicao do celular").unwrap();
    pc.ciclo().unwrap();
    cel.ciclo().unwrap();
    pc.ciclo().unwrap();
    cel.ciclo().unwrap();
    pc.ciclo().unwrap();
    assert_eq!(
        fs::read_to_string(cofre_cel.path().join("a.md")).unwrap(),
        "edicao do pc"
    );
    let conflito = fs::read_dir(cofre_pc.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.contains("(conflito celular"))
        .expect("o pc recebeu a cópia de conflito");
    assert_eq!(
        fs::read_to_string(cofre_pc.path().join(conflito)).unwrap(),
        "edicao do celular"
    );

    // apagar no celular: o pc manda para a lixeira dele
    fs::remove_file(cofre_cel.path().join("pasta/b.md")).unwrap();
    cel.ciclo().unwrap();
    pc.ciclo().unwrap();
    assert!(!cofre_pc.path().join("pasta/b.md").exists());
    let mut lixo = Vec::new();
    ler_tudo(&cofre_pc.path().join(".trash"), &mut lixo);
    assert!(String::from_utf8_lossy(&lixo).contains("outra nota"));

    // o servidor guardou só texto cifrado
    let mut tudo = Vec::new();
    ler_tudo(a.dados.path(), &mut tudo);
    let s = String::from_utf8_lossy(&tudo);
    for proibido in ["secreto", "outra nota", "edicao do", "a.md", "b.md"] {
        assert!(
            !s.contains(proibido),
            "o servidor guardou '{proibido}' em claro"
        );
    }
}

#[test]
fn servidor_fora_do_ar_nao_apaga_nem_perde_nada() {
    let mut a = Ambiente::novo();
    let chave = ChaveCofre::nova();
    let (mut pc, cofre, _e) = sinc(&a, "pc", &chave);
    fs::write(cofre.path().join("n.md"), "nota").unwrap();
    pc.ciclo().unwrap();
    a.parar();
    fs::write(cofre.path().join("n.md"), "editada sem servidor").unwrap();
    fs::write(cofre.path().join("nova.md"), "criada sem servidor").unwrap();
    assert!(matches!(pc.ciclo(), Err(ErroSync::Remoto(_))));
    assert_eq!(
        fs::read_to_string(cofre.path().join("n.md")).unwrap(),
        "editada sem servidor"
    );
    assert_eq!(
        fs::read_to_string(cofre.path().join("nova.md")).unwrap(),
        "criada sem servidor"
    );
}

#[test]
fn token_errado_no_cliente_vira_erro_de_autorizacao() {
    let a = Ambiente::novo();
    let chave = ChaveCofre::nova();
    let mut cfg = a.config("pc");
    cfg.token = "nst_errado".into();
    let cofre = tempfile::tempdir().unwrap();
    let estado = tempfile::tempdir().unwrap();
    let mut s = Sincronizador::novo(
        Opcoes {
            cofre: cofre.path().to_path_buf(),
            estado: estado.path().join("s.redb"),
            aparelho: "pc".into(),
        },
        chave,
        RemotoHttp::novo(&cfg).unwrap(),
    )
    .unwrap();
    let e = s.ciclo().unwrap_err();
    assert_eq!(e, ErroSync::Remoto(nota_sync::ErroRemoto::NaoAutorizado));
}

#[test]
fn token_criado_com_o_servidor_no_ar_vale_na_hora() {
    let a = Ambiente::novo();
    let ag = agente();
    let novo = tokens::novo(a.dados.path(), "tablet").unwrap();
    let r = ag
        .get(a.url("/v1/mudancas?desde=0"))
        .header("Authorization", format!("Bearer {novo}"))
        .call();
    assert_eq!(status(r), 200);
}

#[test]
fn servidor_reiniciado_mantem_o_indice_e_os_blobs() {
    let mut a = Ambiente::novo();
    let chave = ChaveCofre::nova();
    let (mut pc, cofre, estado) = sinc(&a, "pc", &chave);
    fs::write(cofre.path().join("n.md"), "nota").unwrap();
    pc.ciclo().unwrap();
    drop(pc);
    a.parar();
    a.servidor = Some(
        ServidorLocal::iniciar(ConfigServidor {
            dados: a.dados.path().to_path_buf(),
            escutar: "127.0.0.1:0".parse().unwrap(),
            permitir_todas_interfaces: false,
        })
        .unwrap(),
    );
    // um aparelho novo, do zero, recebe a nota do servidor reiniciado
    let (mut cel, cofre2, _e) = sinc(&a, "celular", &chave);
    cel.ciclo().unwrap();
    assert_eq!(
        fs::read_to_string(cofre2.path().join("n.md")).unwrap(),
        "nota"
    );
    drop(estado);
}

#[test]
fn todas_as_interfaces_sao_recusadas_sem_a_flag() {
    let dados = tempfile::tempdir().unwrap();
    for addr in ["0.0.0.0:0", "[::]:0"] {
        let r = ServidorLocal::iniciar(ConfigServidor {
            dados: dados.path().to_path_buf(),
            escutar: addr.parse().unwrap(),
            permitir_todas_interfaces: false,
        });
        assert!(r.is_err(), "{addr}");
    }
    // o binário também recusa, e sai sem subir
    let bin = env!("CARGO_BIN_EXE_nota-servidor");
    let saida = Command::new(bin)
        .args(["serve", "--dados"])
        .arg(dados.path())
        .args(["--escutar", "0.0.0.0:0"])
        .output()
        .unwrap();
    assert!(!saida.status.success());
    assert!(String::from_utf8_lossy(&saida.stderr).contains("recusado"));
}

#[test]
fn subcomando_token_novo_imprime_o_token_e_ele_funciona() {
    let dados = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_nota-servidor");
    let saida = Command::new(bin)
        .args(["token", "novo", "celular", "--dados"])
        .arg(dados.path())
        .output()
        .unwrap();
    assert!(saida.status.success());
    let texto = String::from_utf8_lossy(&saida.stdout).into_owned();
    let token = texto
        .lines()
        .find(|l| l.starts_with("nst_"))
        .expect("o token foi impresso")
        .to_string();
    // o arquivo só tem o hash
    let arq = fs::read_to_string(tokens::arquivo(dados.path())).unwrap();
    assert!(!arq.contains(&token));
    // repetir o nome é recusado
    let de_novo = Command::new(bin)
        .args(["token", "novo", "celular", "--dados"])
        .arg(dados.path())
        .output()
        .unwrap();
    assert!(!de_novo.status.success());
    // o servidor aceita o token
    let s = ServidorLocal::iniciar(ConfigServidor {
        dados: dados.path().to_path_buf(),
        escutar: "127.0.0.1:0".parse().unwrap(),
        permitir_todas_interfaces: false,
    })
    .unwrap();
    let r = agente()
        .get(format!("http://{}/v1/mudancas?desde=0", s.endereco()))
        .header("Authorization", format!("Bearer {token}"))
        .call();
    assert_eq!(status(r), 200);
}
