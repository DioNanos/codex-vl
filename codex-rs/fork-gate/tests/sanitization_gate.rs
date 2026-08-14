// codex-rs/fork-gate/tests/sanitization_gate.rs
//
// Gate di sanificazione pre-publish per il fork codex-vl.
//
// PERCHE' E' UN TEST E NON UNA CHECKLIST. La verifica prima di pubblicare
// c'e' gia' — il grep anti-leak manuale nel MERGE_FEATURE_REGISTER, area
// "Fork identity and release safety" — ma la si fa a mente, ricostruendo
// l'elenco dei motivi da cercare. Una lista scritta a mano copre cio' che
// ricordi; un test copre cio' che c'e', e fallisce prima del publish invece
// che dopo. Specchia e adatta il rationale del test published-tree-is-clean
// di nexuscrew (tests/published-tree-is-clean.test.js).
//
// Ogni voce vietata dice PERCHE' e' vietata: un elenco di stringhe senza
// motivo si svuota di senso e prima o poi qualcuno ne toglie una per far
// passare la suite. Un test ausiliario prova che i motivi MORDANO ancora,
// su testo costruito apposta, e che NON mordano i casi legittimi (falsi
// positivi). Un binario inatteso in un file di testo FA FALLIRE, non e'
// saltato in silenzio (lezione del test nexuscrew, DEC3).
//
// Il gate e' ROSSO sul commit di base: le tre classi esistono. Non ripara
// il codice, segnala. Le riparazioni le decide l'operatore.
//
// I needle vietati sono costruiti a FRAMMENTI (mai letterali nel sorgente):
// cosi' il guardiano non si auto-accusa e non porta dentro, per
// definizione, le stesse tracce che cerca.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---- infra ----------------------------------------------------------------

/// Radice del repo: da CARGO_MANIFEST_DIR risale finche' trova `.git`.
fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        if !dir.pop() {
            panic!(
                "radice del repo non trovata (nessun .git) a partire da {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

/// `git -C <root> <args>`: panic se fallisce, restituisce stdout (lossy).
fn git(root: &Path, args: &[&str]) -> String {
    match Command::new("git").arg("-C").arg(root).args(args).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => panic!(
            "git {:?} fallito (status {}): {}",
            args,
            o.status,
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => panic!("git non eseguibile: {e}"),
    }
}

/// Vero se `git rev-parse --verify <ref>` ha successo.
fn ref_exists(root: &Path, refname: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", refname])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Concatena frammenti in una String: un needle costruito non e' mai un
/// letterale nel sorgente del gate.
fn joined(parts: &[&str]) -> String {
    let mut s = String::new();
    for p in parts {
        s.push_str(p);
    }
    s
}

/// Un needle vietato col suo PERCHE'.
struct Vietato {
    needle: String,
    perche: &'static str,
}

// ---- needle vietati (costruiti a frammenti) --------------------------------

/// Attribuzione AI nel corpo dei commit. Classe piu' importante e piu'
/// difficile: vive nella STORIA (git log), non nei file. Guardare solo i
/// file la manca. I commit upstream pubblici prima del fork NON sono nostri
/// e non devono far fallire: vedi il criterio nel test di classe 1.
fn ai_vietati() -> Vec<Vietato> {
    vec![
        Vietato {
            needle: joined(&["Co-", "Authored-", "By"]),
            perche: "trailer git di co-autoria AI nel corpo del commit",
        },
        Vietato {
            needle: joined(&["Generated", " with"]),
            perche: "firma di generazione AI nel corpo del commit",
        },
        Vietato {
            needle: String::from("\u{1F916}"),
            perche: "emoji del robot (attribuzione AI) nel corpo del commit",
        },
    ]
}

/// Handle dell'operatore come PAROLA ISOLATA nel sorgente Rust. Si cerca la
/// parola isolata, non la sottostringa: un'occorrenza dentro una parola
/// piu' lunga, o attaccata a underscore/cifre, non e' leak.
fn op_handle() -> String {
    joined(&["D", "A", "G"])
}

/// Marcatori di audit interni che non appartengono alle note di release
/// tracciate (.release/*.md): il register li esclude dal tree pubblico.
fn audit_vietati() -> Vec<Vietato> {
    vec![
        Vietato {
            needle: joined(&["merge", "-feature-", "register"]),
            perche: "marker del register di merge (audit interno) in note di release",
        },
        Vietato {
            needle: joined(&["APPROVE", ":"]),
            perche: "marker di verdetto del register (audit interno) in note di release",
        },
    ]
}

// ---- scansione -------------------------------------------------------------

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// "parola isolata": needle con confini che non sono word-byte (alphanum o
/// underscore) ai due lati. Cosi' le sottostringhe attaccate non matchano.
fn has_isolated(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    let mut from = 0usize;
    while let Some(rel) = h[from..].windows(n.len()).position(|w| w == n) {
        let abs = from + rel;
        let before_ok = abs == 0 || !is_word_byte(h[abs - 1]);
        let after_idx = abs + n.len();
        let after_ok = after_idx >= h.len() || !is_word_byte(h[after_idx]);
        if before_ok && after_ok {
            return true;
        }
        from = abs + n.len();
    }
    false
}

/// Colpe di un file di testo (rel path + byte). Un byte NUL => binario
/// inatteso in un file di testo: COLPEVOLE, non saltato (mai verde per
/// assenza di ispezione). `isolated` decide se il needle va cercato come
/// parola isolata o come sottostringa.
fn scan_text(rel: &str, bytes: &[u8], vietati: &[Vietato], isolated: bool) -> Vec<String> {
    let mut out = Vec::new();
    if bytes.contains(&0u8) {
        out.push(format!(
            "{rel}: file binario (byte NUL) in un file di testo atteso — ispezionare, non saltare"
        ));
        return out;
    }
    let text = String::from_utf8_lossy(bytes);
    for (i, line) in text.lines().enumerate() {
        for v in vietati {
            let hit = if isolated {
                has_isolated(line, &v.needle)
            } else {
                line.contains(v.needle.as_str())
            };
            if hit {
                out.push(format!("{rel}:{}: {}", i + 1, v.perche));
            }
        }
    }
    out
}

// ---- CLASSE 1: attribuzione AI nella STORIA del fork ----------------------

#[test]
fn classe1_attribuzione_ai_assente_dalla_storia_del_fork() {
    let root = repo_root();

    // CRITERIO (dichiarato): un commit e' "nostro" se e' raggiungibile da
    // HEAD ma NON da upstream/main. Usa l'ancestry di git — verita' di terra,
    // non euristiche su autore o data. I commit upstream pubblici prima del
    // fork stanno nella storia di upstream/main e sono quindi esclusi: e'
    // proprio cio' che li distingue dai nostri.
    //
    // Perche' non autore: il merge commit che porta upstream e' comunque
    // nostro (autore del fork), e upstream contribuisce con email eterogenee
    // — l'autore e' un proxy, non la verita'. Perche' non data: la data del
    // fork non e' un confine netto (mergiamo upstream continuamente).
    // L'ancestry lo e': un commit o sta nella storia pubblica upstream, o no.
    //
    // Dipendenza: upstream/main deve essere presente (`git fetch upstream`).
    // Se manca, il gate FALLISCE forte invece di passare verde per assenza di
    // ispezione. Un upstream/main stallo genera SOLO falsi positivi (piu'
    // commit classificati come nostri, mai meno): e' safe-failing, non silente.
    assert!(
        ref_exists(&root, "upstream/main"),
        "upstream/main mancante: il criterio 'nostro vs upstream' non e' applicabile. \
         Esegui `git fetch upstream`. Il gate non passa verde per assenza di ispezione."
    );

    // Un'unica invocazione: %x00 separa l'hash dal corpo; il corpo e' NUL-free,
    // lo split e' robusto. Molto piu' veloce di un git log per commit.
    let raw = git(
        &root,
        &["log", "HEAD", "--not", "upstream/main", "--format=%H%x00%B%x00"],
    );
    let parts: Vec<&str> = raw.split('\0').collect();
    let needles = ai_vietati();
    let mut colpe: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 1 < parts.len() {
        let sha = parts[i].trim();
        let body = parts[i + 1];
        i += 2;
        if sha.is_empty() {
            continue;
        }
        let hit: Vec<&str> = needles
            .iter()
            .filter(|n| body.contains(n.needle.as_str()))
            .map(|v| v.perche)
            .collect();
        if !hit.is_empty() {
            let short = &sha[..12.min(sha.len())];
            let subj = body.lines().next().unwrap_or("").trim();
            colpe.push(format!(
                "commit NOSTRO {short} «{subj}»: {}",
                hit.join("; ")
            ));
        }
    }
    assert!(
        colpe.is_empty(),
        "la storia del fork (HEAD --not upstream/main) non deve contenere attribuzione AI:\n  {}",
        colpe.join("\n  ")
    );
}

// ---- CLASSE 2: handle dell'operatore nel sorgente Rust --------------------

#[test]
fn classe2_handle_operatore_assente_dal_rust_tracciato() {
    let root = repo_root();
    let handle = op_handle();
    let tracked: Vec<String> = git(&root, &["ls-files"])
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| s.ends_with(".rs"))
        .collect();
    let mut per_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut binari: Vec<String> = Vec::new();
    for rel in &tracked {
        let path = root.join(rel);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                binari.push(format!("{rel}: non leggibile: {e}"));
                continue;
            }
        };
        if bytes.contains(&0u8) {
            binari.push(format!(
                "{rel}: file binario (byte NUL) in un sorgente .rs — ispezionare, non saltare"
            ));
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut n = 0usize;
        for line in text.lines() {
            if has_isolated(line, &handle) {
                n += 1;
            }
        }
        if n > 0 {
            *per_file.entry(rel.clone()).or_insert(0) += n;
        }
    }
    let mut colpe: Vec<String> = binari;
    for (rel, n) in &per_file {
        colpe.push(format!("{rel}: {n}x handle dell'operatore come parola isolata"));
    }
    assert!(
        colpe.is_empty(),
        "il sorgente Rust tracciato non deve contenere l'handle dell'operatore come parola isolata:\n  {}",
        colpe.join("\n  ")
    );
}

// ---- CLASSE 3: marker di audit interni nelle note di release ---------------

#[test]
fn classe3_marker_audit_assenti_dalle_release_notes() {
    let root = repo_root();
    let vietati = audit_vietati();
    let tracked: Vec<String> = git(&root, &["ls-files"])
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with(".release/") && s.ends_with(".md"))
        .collect();
    let mut colpe: Vec<String> = Vec::new();
    for rel in &tracked {
        let path = root.join(rel);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                colpe.push(format!("{rel}: non leggibile: {e}"));
                continue;
            }
        };
        colpe.extend(scan_text(rel, &bytes, &vietati, false));
    }
    assert!(
        colpe.is_empty(),
        "le note di release tracciate (.release/*.md) non devono contenere marker di audit interni:\n  {}",
        colpe.join("\n  ")
    );
}

// ---- il guardiano del guardiano: i motivi mordano ancora -------------------

#[test]
fn i_motivi_mordano_ancora() {
    // Se qualcuno svuotasse i needle, i test sopra passerebbero sempre: verde
    // per assenza di controlli invece che per assenza di tracce. Qui si prova
    // che i motivi mordano su testo costruito, e che NON mordano i casi
    // legittimi (falsi positivi). Un guardiano che grida al lupo si smette
    // di ascoltarlo.
    let handle = op_handle();

    // handle: parola isolata morde.
    let bad_rs = format!("design directive 2026-05-15: {handle} observed Nilo stuck");
    assert!(
        has_isolated(&bad_rs, &handle),
        "l'handle deve riconoscere la parola isolata"
    );
    // handle: sottostringa NON isolata NON morde (falsi positivi).
    for ok in ["DAGGER", "FOO_DAG_BAR", "CDAG", "myDAG2"] {
        assert!(
            !has_isolated(ok, &handle),
            "falso positivo su sottostringa non isolata: {ok}"
        );
    }

    // attribuzione AI: tutti i needle mordono nel corpo di un commit.
    let ai = ai_vietati();
    let bad_body = format!(
        "fix: x\n\n{}: Any Assistant <noreply@example.com>\n{} [tool]\n{}\n",
        ai[0].needle, ai[1].needle, ai[2].needle
    );
    assert!(
        ai.iter().all(|v| bad_body.contains(v.needle.as_str())),
        "ogni needle di attribuzione AI deve mordere"
    );

    // marker di audit: tutti i needle mordono in una riga di release.
    let am = audit_vietati();
    let bad_release = format!("release: {} {}verdict:codex-vl-x\n", am[0].needle, am[1].needle);
    assert!(
        am.iter().all(|v| bad_release.contains(v.needle.as_str())),
        "ogni needle di audit deve mordere"
    );

    // L'identita' PUBBLICA del progetto NON e' un leak: non deve essere
    // flaggata. (dominio, mail e repo del fork sono dichiarati e pubblici.)
    for identity in ["dev@mmmbuto.com", "mmmbuto", "DioNanos/codex-vl"] {
        assert!(
            !has_isolated(identity, &handle),
            "l'identita' pubblica del progetto non e' un leak: {identity}"
        );
    }
}

// ---- CONTROLLO NEGATIVO: un binario inatteso FA FALLIRE, non e' saltato ----

#[test]
fn un_binario_inatteso_in_testo_fa_fallire_non_essere_saltato() {
    // Lezione del test nexuscrew (DEC3): un binario non dichiarato NON deve
    // essere saltato in silenzio. Un NUL in un file di testo atteso produce
    // una colpa "binario da ispezionare", mai verde.
    let v = vec![Vietato {
        needle: op_handle(),
        perche: "handle operatore (di prova)",
    }];
    let f = scan_text("src/fittizio.dat", b"before\x00after", &v, true);
    assert!(
        !f.is_empty(),
        "un binario (NUL) in un file di testo atteso deve essere colpevole, non saltato"
    );
    assert!(
        f.iter().any(|s| s.contains("binario") && s.contains("ispezionare")),
        "segnalato come binario da ispezionare: {f:?}"
    );
    // Anche se contiene il needle: il gate NON lo nasconde nel verde —
    // restituisce la colpa binaria (ispezionare), non la salta.
    let g = scan_text("src/fittizio.rs", b"x\x00y", &v, true);
    assert!(
        g.iter().any(|s| s.contains("binario")),
        "il NUL prevale: ispezionare, non verde: {g:?}"
    );
}