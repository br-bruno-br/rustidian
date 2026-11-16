//! Cenários do executor, um por vez e sem sorte. A propriedade aleatória está em
//! `propriedade.rs`.

mod comum;

use std::sync::{Arc, Mutex};

use comum::{Mundo, clone_chave};
use nota_cripto::ChaveCofre;
use nota_sync::ErroSync;
use nota_sync::Remoto;
use nota_sync::memoria::{Chamada, Falha, Quando};

fn estabilizar(aps: &mut [&mut comum::Aparelho]) {
    for _ in 0..20 {
        let mut quieto = true;
        for a in aps.iter_mut() {
            let r = a.ciclo().unwrap();
            quieto &= r.acoes == 0;
        }
        if quieto {
            return;
        }
    }
    panic!("não estabilizou em 20 rodadas");
}

#[test]
fn dois_aparelhos_convergem_e_o_servidor_nao_ve_o_texto() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("a.md", "texto secreto da nota A");
    a.escrever("pasta/b.md", "segunda nota");
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(b.ler("a.md").as_deref(), Some("texto secreto da nota A"));
    assert_eq!(b.ler("pasta/b.md").as_deref(), Some("segunda nota"));

    b.escrever("a.md", "editada no celular");
    estabilizar(&mut [&mut b, &mut a]);
    assert_eq!(a.ler("a.md").as_deref(), Some("editada no celular"));
    assert_eq!(a.vivos(), b.vivos());
    assert!(a.conflitos().is_empty() && b.conflitos().is_empty());

    for blob in m.central.blobs() {
        let s = String::from_utf8_lossy(&blob);
        assert!(!s.contains("secreto") && !s.contains("a.md") && !s.contains("celular"));
    }
}

#[test]
fn ciclo_sem_mudanca_nao_faz_nada() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    a.escrever("a.md", "x");
    assert_eq!(a.ciclo().unwrap().enviados, 1);
    let r = a.ciclo().unwrap();
    assert_eq!(r.acoes, 0, "{r:?}");
    assert!(r.recusados.is_empty());
}

#[test]
fn mudou_nos_dois_lados_remoto_vira_principal_e_local_vira_conflito() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "base");
    estabilizar(&mut [&mut a, &mut b]);
    a.escrever("n.md", "versao do pc");
    b.escrever("n.md", "versao do celular");
    a.ciclo().unwrap(); // o pc publica primeiro
    b.ciclo().unwrap();
    assert_eq!(b.ler("n.md").as_deref(), Some("versao do pc"));
    let conflitos = b.conflitos();
    assert_eq!(conflitos.len(), 1, "{conflitos:?}");
    assert!(conflitos[0].starts_with("n (conflito celular 2026-11-15 "));
    assert_eq!(b.ler(&conflitos[0]).as_deref(), Some("versao do celular"));
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(a.vivos(), b.vivos());
    assert_eq!(a.conflitos().len(), 1);
}

#[test]
fn apagado_x_editado_a_edicao_vence() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "base");
    estabilizar(&mut [&mut a, &mut b]);
    a.apagar("n.md");
    b.escrever("n.md", "editada");
    a.ciclo().unwrap(); // lápide no servidor
    b.ciclo().unwrap(); // a edição vence a lápide
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(a.ler("n.md").as_deref(), Some("editada"));
    assert_eq!(b.ler("n.md").as_deref(), Some("editada"));
    assert!(a.conflitos().is_empty());
}

#[test]
fn editado_x_apagado_o_arquivo_volta() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "base");
    estabilizar(&mut [&mut a, &mut b]);
    b.apagar("n.md");
    b.ciclo().unwrap(); // lápide publicada
    a.escrever("n.md", "editada no pc");
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(b.ler("n.md").as_deref(), Some("editada no pc"));
    assert_eq!(a.vivos(), b.vivos());
}

#[test]
fn apagar_remoto_manda_para_a_lixeira_nunca_apaga() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("p/n.md", "conteudo precioso");
    estabilizar(&mut [&mut a, &mut b]);
    a.apagar("p/n.md");
    estabilizar(&mut [&mut a, &mut b]);
    assert!(b.ler("p/n.md").is_none());
    assert!(b.lixeira().contains(&"conteudo precioso".to_string()));
    assert!(a.vivos().is_empty() && b.vivos().is_empty());
}

#[test]
fn apagado_nos_dois_lados_nao_gera_erro_nem_conflito() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "x");
    estabilizar(&mut [&mut a, &mut b]);
    a.apagar("n.md");
    b.apagar("n.md");
    estabilizar(&mut [&mut a, &mut b]);
    assert!(a.vivos().is_empty() && b.vivos().is_empty());
}

#[test]
fn criado_igual_nos_dois_lados_nao_e_conflito() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "mesmo texto");
    b.escrever("n.md", "mesmo texto");
    estabilizar(&mut [&mut a, &mut b]);
    assert!(a.conflitos().is_empty() && b.conflitos().is_empty());
    assert_eq!(a.vivos(), b.vivos());
}

#[test]
fn arquivo_mudou_entre_a_varredura_e_a_escrita_vira_conflito() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "v1");
    estabilizar(&mut [&mut a, &mut b]);
    a.escrever("n.md", "v2 do pc");
    a.ciclo().unwrap();
    // No ciclo do celular, o usuário edita DEPOIS da varredura (no primeiro download).
    let arq = b.caminho("n.md");
    let feito = Arc::new(Mutex::new(false));
    let f2 = feito.clone();
    b.sinc.remoto().definir_gancho(Some(Box::new(move |c| {
        if c == Chamada::BaixarBlob {
            let mut f = f2.lock().unwrap();
            if !*f {
                std::fs::write(&arq, "edicao no meio do ciclo").unwrap();
                *f = true;
            }
        }
    })));
    b.ciclo().unwrap();
    b.sinc.remoto().definir_gancho(None);
    assert!(*feito.lock().unwrap());
    assert_eq!(b.ler("n.md").as_deref(), Some("v2 do pc"));
    let conflitos = b.conflitos();
    assert_eq!(conflitos.len(), 1);
    assert_eq!(
        b.ler(&conflitos[0]).as_deref(),
        Some("edicao no meio do ciclo")
    );
}

#[test]
fn queda_depois_da_publicacao_nao_duplica_nem_perde() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "texto");
    // chamadas do ciclo: 0 = mudancas, 1 = enviar_blob, 2 = publicar
    a.sinc.remoto().injetar(vec![Falha {
        n: 2,
        quando: Quando::Depois,
    }]);
    assert!(matches!(a.ciclo(), Err(ErroSync::Remoto(_))));
    a.sinc.remoto().injetar(vec![]);
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(b.ler("n.md").as_deref(), Some("texto"));
    assert!(a.conflitos().is_empty() && b.conflitos().is_empty());
    let id = m.chave.id_de("n.md").unwrap().0;
    assert_eq!(
        m.central.item(&id).unwrap().versao,
        1,
        "publicou duas vezes"
    );
}

#[test]
fn queda_antes_de_publicar_retoma_do_diario() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    a.escrever("n.md", "texto");
    a.sinc.remoto().injetar(vec![Falha {
        n: 2,
        quando: Quando::Antes,
    }]);
    assert!(a.ciclo().is_err());
    assert!(!a.sinc.estado().diario_vazio().unwrap());
    a.sinc.remoto().injetar(vec![]);
    a.ciclo().unwrap();
    assert!(a.sinc.estado().diario_vazio().unwrap());
    let id = m.chave.id_de("n.md").unwrap().0;
    assert_eq!(m.central.item(&id).unwrap().versao, 1);
}

#[test]
fn queda_de_processo_em_cada_ponto_retoma_sem_perder_nem_duplicar() {
    for n in 0..4 {
        for depois in [false, true] {
            let m = Mundo::novo();
            let mut a = m.aparelho("pc");
            let mut b = m.aparelho("celular");
            a.escrever("a.md", "A1");
            a.escrever("b.md", "B1");
            estabilizar(&mut [&mut a, &mut b]);
            // edições dos dois lados e uma apagada
            a.escrever("a.md", "A2");
            b.escrever("a.md", "A2 do celular");
            a.apagar("b.md");
            a.escrever("c.md", "C1");
            a.ciclo().unwrap();
            b.sinc.injetar_queda(n, depois);
            let r = b.ciclo();
            // o plano tem 4 intenções (conflito, baixar a.md, apagar b.md, baixar c.md)
            assert_eq!(
                r.unwrap_err(),
                ErroSync::Interrompido,
                "n={n} depois={depois}"
            );
            b.sinc.limpar_queda();
            b.reiniciar();
            estabilizar(&mut [&mut b, &mut a]);
            assert_eq!(a.vivos(), b.vivos(), "n={n} depois={depois}");
            assert_eq!(
                b.ler("a.md").as_deref(),
                Some("A2"),
                "n={n} depois={depois}"
            );
            assert_eq!(b.ler("c.md").as_deref(), Some("C1"));
            assert!(b.ler("b.md").is_none());
            let conflitos = b.conflitos();
            assert_eq!(conflitos.len(), 1, "{conflitos:?} n={n} depois={depois}");
            assert_eq!(b.ler(&conflitos[0]).as_deref(), Some("A2 do celular"));
            assert!(b.lixeira().contains(&"B1".to_string()));
        }
    }
}

#[test]
fn blob_de_outro_id_e_recusado_e_nada_e_escrito() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    a.escrever("n.md", "texto");
    a.ciclo().unwrap();
    // O servidor (ou um invasor) aponta o item de "x.md" para o blob de "n.md".
    let id_n = m.chave.id_de("n.md").unwrap().0;
    let id_x = m.chave.id_de("x.md").unwrap().0;
    let blob = m.central.item(&id_n).unwrap().hash_blob.unwrap();
    let mut srv = m.central.outro_aparelho();
    srv.publicar(&id_x, 0, &blob, false).unwrap().unwrap();
    let mut b = m.aparelho("celular");
    let r = b.ciclo().unwrap();
    assert!(!r.recusados.is_empty(), "{r:?}");
    assert!(b.ler("x.md").is_none());
    assert_eq!(b.ler("n.md").as_deref(), Some("texto"));
}

#[test]
fn chave_errada_nao_escreve_nem_apaga_nada() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    a.escrever("n.md", "texto");
    a.ciclo().unwrap();
    let outra = ChaveCofre::nova();
    let mut b = comum::Aparelho::novo("intruso", &m.central, &outra, m.tempo.clone());
    b.escrever("meu.md", "meu");
    let r = b.ciclo().unwrap();
    assert!(!r.recusados.is_empty());
    assert_eq!(b.vivos().len(), 1);
    assert_eq!(b.ler("meu.md").as_deref(), Some("meu"));
}

#[test]
fn guarda_contra_exclusao_em_massa() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    for n in 0..30 {
        a.escrever(&format!("n{n}.md"), &format!("nota {n}"));
    }
    a.ciclo().unwrap();
    // cofre "desmontado": a pasta existe mas está vazia
    for n in 0..30 {
        std::fs::remove_file(a.caminho(&format!("n{n}.md"))).unwrap();
    }
    let r = a.ciclo();
    assert!(matches!(r, Err(ErroSync::ExclusaoEmMassa { .. })), "{r:?}");
    let id = m.chave.id_de("n0.md").unwrap().0;
    assert!(!m.central.item(&id).unwrap().apagado);
    a.sinc.permitir_exclusao_em_massa(true);
    a.ciclo().unwrap();
    assert!(m.central.item(&id).unwrap().apagado);
}

#[test]
fn cofre_que_nao_existe_e_erro() {
    let m = Mundo::novo();
    let d = tempfile::tempdir().unwrap();
    let r = nota_sync::Sincronizador::novo(
        nota_sync::Opcoes {
            cofre: d.path().join("nao-existe"),
            estado: d.path().join("s.redb"),
            aparelho: "pc".into(),
        },
        clone_chave(&m.chave),
        m.central.outro_aparelho(),
    );
    assert!(r.is_err());
}

#[test]
fn maiusculas_e_minusculas_nao_perdem_nada() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("nota.md", "do pc");
    b.escrever("Nota.md", "do celular");
    estabilizar(&mut [&mut a, &mut b]);
    // Os dois textos continuam vivos em cada aparelho e ninguém fica com dois caminhos que
    // só diferem na caixa.
    for ap in [&a, &b] {
        let v = ap.vivos();
        let mut dobrados: Vec<String> = v.keys().map(|k| k.to_lowercase()).collect();
        dobrados.sort();
        dobrados.dedup();
        assert_eq!(dobrados.len(), v.len(), "{:?}", v.keys());
    }
    let tudo: Vec<String> = a
        .vivos()
        .into_values()
        .chain(b.vivos().into_values())
        .collect();
    assert!(tudo.contains(&"do pc".to_string()));
    assert!(tudo.contains(&"do celular".to_string()));
}

#[cfg(unix)]
#[test]
fn symlink_no_cofre_nao_e_seguido_nem_escrito() {
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    let fora = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(fora.path(), b.caminho("pasta")).unwrap();
    a.escrever("pasta/n.md", "texto");
    a.ciclo().unwrap();
    let r = b.ciclo().unwrap();
    assert!(
        std::fs::read_dir(fora.path()).unwrap().next().is_none(),
        "{r:?}"
    );
}

#[test]
fn apagar_nao_leva_para_a_lixeira_o_arquivo_editado_depois_da_varredura() {
    use nota_sync::estado::Intencao;
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "base");
    estabilizar(&mut [&mut a, &mut b]);
    a.apagar("n.md");
    a.ciclo().unwrap(); // lápide no servidor
    let arq = b.caminho("n.md");
    let editou = Arc::new(Mutex::new(false));
    let e2 = editou.clone();
    b.sinc.definir_gancho_efeito(Some(Box::new(move |_, i| {
        if matches!(i, Intencao::ApagarLocal { .. }) {
            std::fs::write(&arq, "editada entre a varredura e a lixeira").unwrap();
            *e2.lock().unwrap() = true;
        }
    })));
    b.ciclo().unwrap();
    b.sinc.definir_gancho_efeito(None);
    assert!(*editou.lock().unwrap());
    assert_eq!(
        b.ler("n.md").as_deref(),
        Some("editada entre a varredura e a lixeira")
    );
    assert!(b.lixeira().is_empty());
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(
        a.ler("n.md").as_deref(),
        Some("editada entre a varredura e a lixeira")
    );
}

#[test]
fn enviar_manda_o_que_esta_no_disco_na_hora_do_envio() {
    use nota_sync::estado::Intencao;
    let m = Mundo::novo();
    let mut a = m.aparelho("pc");
    let mut b = m.aparelho("celular");
    a.escrever("n.md", "primeira");
    let arq = a.caminho("n.md");
    a.sinc.definir_gancho_efeito(Some(Box::new(move |_, i| {
        if matches!(i, Intencao::Enviar { .. }) {
            std::fs::write(&arq, "segunda, escrita depois da varredura").unwrap();
        }
    })));
    a.ciclo().unwrap();
    a.sinc.definir_gancho_efeito(None);
    estabilizar(&mut [&mut a, &mut b]);
    assert_eq!(
        b.ler("n.md").as_deref(),
        Some("segunda, escrita depois da varredura")
    );
    assert!(a.conflitos().is_empty() && b.conflitos().is_empty());
}
