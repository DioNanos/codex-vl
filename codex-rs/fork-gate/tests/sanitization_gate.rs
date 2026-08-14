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

/// Come `git` ma non panic: ritorna Some(stdout) se ok, None se fallisce. Per
/// comandi opzionali nel fixture (es. `update-ref -d` di un ref che puo' non
/// esserci): non far cadere il test per un ref assente.
fn git_try(root: &Path, args: &[&str]) -> Option<String> {
    match Command::new("git").arg("-C").arg(root).args(args).output() {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => None,
    }
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

/// Vero se `haystack` contiene `needle`, case-insensitive (ASCII). Per i
/// trailer git e le firme di generazione: il trailer `co-authored-by`
/// scritto in minuscolo da un tool diverso deve mordere quanto `Co-Authored-By`
/// o `CO-AUTHORED-BY`. NON si usa per l'handle dell'operatore, che resta
/// case-sensitive (vedi `op_handle`): `dag` minuscolo e' un'altra cosa
/// (directed acyclic graph) e intercettarlo sarebbe gridare al lupo.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(needle.to_ascii_lowercase().as_str())
}

// ---- autenticazione del remote upstream ------------------------------------

// Repository upstream atteso: il CRITERIO e' scritto qui nel test, non
// assunto dall'ambiente. codex-vl e' un fork DOWNSTREAM di openai/codex su
// GitHub: solo quello e' il riferimento contro cui ha senso misurare i commit
// "nostri". Si accetta sia HTTPS sia SSH dello STESSO repo: l'identita' e'
// "host/owner/repo", il protocollo e' un dettaglio che non deve far fallire
// un remote legittimo. Ma un remote che punta a un ALTRO repo — un fork
// qualunque, un mirror, un repo sbagliato — NON e' ammesso. Il difetto che
// questo chiude: il gate verificava che l'insieme dei commit "nostri" fosse
// NON VUOTO, non che fosse quello GIUSTO. Se il remote punta altrove,
// l'insieme risulta non vuoto e SBAGLIATO: superata la guardia sul vuoto, il
// confronto avviene contro il riferimento sbagliato e un commit con
// attribuzione AI resterebbe nascosto (presente nel remote sbagliato =>
// classificato "upstream" => non ispezionato). La stessa forma di ieri —
// "il gate seleziona qualcosa" contro "il gate seleziona cio' che dichiara
// di guardare". Ora il gate autentica il remote PRIMA di fidarsene: se non
// e' quello atteso, FALLISCE dicendo perche' — non prova a indovinare e non
// degrada silenziosamente.
const UPSTREAM_HOST: &str = "github.com";
const UPSTREAM_REPO: &str = "openai/codex";

/// Normalizza un URL di remote git alla forma canonica "host/owner/repo"
/// (lowercase, senza suffisso .git). Riconosce le forme HTTPS
/// (https://host/owner/repo[.git]), SSH con scheme (ssh://git@host/owner/repo)
/// e scp-like (git@host:owner/repo). Restituisce None se la forma non e'
/// riconosciuta: un URL non interpretabile e' comunque un fail del gate, non
/// un degrade silenzioso.
fn canonical_remote(url: &str) -> Option<String> {
    let u = url.trim();
    let u = u.strip_suffix(".git").unwrap_or(u);
    // scheme://... (https, ssh): tutto dopo "://", scartando l'eventuale
    // userinfo "user@host" — si tiene da host in poi.
    if let Some(idx) = u.find("://") {
        let after = &u[idx + 3..];
        let after = after.rfind('@').map(|a| &after[a + 1..]).unwrap_or(after);
        return Some(after.trim_end_matches('/').to_ascii_lowercase());
    }
    // forma scp-like: git@host:owner/repo
    if let Some(at) = u.rfind('@') {
        let after = &u[at + 1..];
        if let Some(c) = after.find(':') {
            let host = &after[..c];
            let tail = &after[c + 1..];
            return Some(format!("{host}/{}", tail.trim_end_matches('/')).to_ascii_lowercase());
        }
    }
    None
}

// ---- provenienza dei ref: il gate si fida solo se puo' provarla -----------

/// Primo campo fra apici in `s` (il nome di branch/ref in una riga di
/// FETCH_HEAD: `branch 'main' of <url>`). I nomi di branch git non contengono
/// apici, quindi il primo paio delimita il nome.
fn first_quoted(s: &str) -> Option<&str> {
    let a = s.find('\'')?;
    let b = s[a + 1..].find('\'')?;
    Some(&s[a + 1..a + 1 + b])
}

/// Provenienza dei ref `refs/remotes/upstream/*` che il gate usa come limite
/// negativo (`HEAD --not <ref>`) per classificare i commit "nostri".
///
/// Autenticare l'URL del remote (la CONFIGURAZIONE) NON basta: `git remote
/// set-url` cambia la configurazione ma NON ripulisce i ref gia' scaricati,
/// che restano quelli presi dal remote precedente. L'URL oggi corretto non
/// prova la provenienza dei ref che quell'URL non ha mai scaricato: un ref
/// stale puo' rendere raggiungibile un commit vietato da `refs/remotes/upstream/*`
/// => il commit e' classificato "upstream" => non ispezionato => NASCOSTO.
/// Misurato: dopo un `set-url`, un ref stale del remote di prima nasconde un
/// commit vietato — un commit selezionato e zero colpe.
///
/// Prova LOCALE, senza rete: l'unico tracciamento che git lascia del remote da
/// cui un ref e' stato scaricato e' il file FETCH_HEAD, scritto a ogni
/// `git fetch`. Ogni riga registra `<sha> ... <branch> 'NAME' of <url>`. Se
/// l'URL registrato e' quello upstream atteso E ogni ref che il gate usa e'
/// elencato in quel FETCH_HEAD con lo STESSO sha, i ref sono provati come
/// provenienti dal remote dichiarato. Se manca FETCH_HEAD, se l'URL non
/// corrisponde, o se un ref non e' coperto (fetch parziale / FETCH_HEAD
/// sovrascritto da un altro remote) o ha sha diverso (ref riposizionato dopo
/// l'ultima fetch provata), la provenienza NON e' provata: il gate FALLISCE
/// dicendolo, non indovina, non degrada.
///
/// L'operatore soddisfa la condizione con `git fetch upstream` (fetch piena
/// dal remote attualmente configurato), che rinfresca ref + FETCH_HEAD dallo
/// STESSO url. Un gate che facesse lui il fetch dipenderebbe dalla rete e
/// cadrebbe a caso: il gate PRETENDE la condizione locale e dice come
/// soddisfarla.
///
/// `expected_canonical` e' la forma "host/owner/repo" attesa (la stessa del
/// controllo di URL di config). Ritorna `Ok(())` se ogni ref e' provato,
/// `Err(failures)` altrimenti.
fn provenienza_refs_provata(root: &Path, expected_canonical: &str) -> Result<(), Vec<String>> {
    // Ref usati dal gate: refs/remotes/upstream/* -> nome -> sha.
    let out = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/remotes/upstream",
        ],
    );
    let mut refs: BTreeMap<String, String> = BTreeMap::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.splitn(2, ' ');
        let rname = it.next().unwrap_or("");
        let sha = it.next().unwrap_or("");
        if rname.is_empty() || sha.is_empty() {
            continue;
        }
        let name = rname
            .strip_prefix("refs/remotes/upstream/")
            .unwrap_or(rname);
        refs.insert(name.to_string(), sha.to_string());
    }
    if refs.is_empty() {
        // Il gate ha gia' assertito refs non-vuoto prima di chiamare; se qui e'
        // vuoto la provenienza e' comunque non applicabile: fail, non verde.
        return Err(vec![
            "nessun ref refs/remotes/upstream/*: il criterio 'nostro vs upstream' non e' \
             applicabile"
                .into(),
        ]);
    }

    // FETCH_HEAD: path robusto anche in worktree (common dir).
    let fh_rel = git(root, &["rev-parse", "--git-path", "FETCH_HEAD"])
        .trim()
        .to_string();
    let fh_path = if Path::new(&fh_rel).is_absolute() {
        PathBuf::from(&fh_rel)
    } else {
        root.join(&fh_rel)
    };
    let content = match fs::read(&fh_path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(_) => {
            return Err(vec![format!(
                "FETCH_HEAD non leggibile ({}): nessuna fetch registrata, la provenienza dei \
                 ref non e' provata. Esegui `git fetch upstream`.",
                fh_path.display()
            )]);
        }
    };

    // FETCH_HEAD -> nome -> (sha, canonical url). Una riga senza ` of <url>`
    // (fetch di un oggetto diretto, senza ref) non prova un ref: la si salta.
    let mut fh: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    for raw in content.lines() {
        let fields: Vec<&str> = raw.split('\t').collect();
        if fields.is_empty() {
            continue;
        }
        let sha = fields[0].trim();
        let last = *fields.last().unwrap_or(&"");
        let url = match raw.rsplit_once(" of ") {
            Some((_, u)) => u.trim(),
            None => continue,
        };
        let name = match first_quoted(last) {
            Some(n) => n,
            None => continue,
        };
        if sha.is_empty() {
            continue;
        }
        fh.entry(name.to_string())
            .or_insert((sha.to_string(), canonical_remote(url)));
    }

    // Ogni ref usato deve essere provato: presente in FETCH_HEAD, stesso sha,
    // dal remote atteso. Il primo ref non provato basta a cadere; si
    // raccolgono fino a 5 esempi piu' un conteggio per un messaggio leggibile.
    let mut fail: Vec<String> = Vec::new();
    let mut nfail: usize = 0;
    for (name, ref_sha) in &refs {
        match fh.get(name) {
            None => {
                nfail += 1;
                if fail.len() < 5 {
                    fail.push(format!(
                        "ref refs/remotes/upstream/{name} non e' in FETCH_HEAD: provenienza non \
                         provata (fetch parziale o FETCH_HEAD sovrascritto da un altro remote). \
                         Esegui `git fetch upstream`."
                    ));
                }
            }
            Some((fh_sha, canonical)) => {
                if fh_sha != ref_sha {
                    nfail += 1;
                    if fail.len() < 5 {
                        fail.push(format!(
                            "ref refs/remotes/upstream/{name}: sha nel ref ({ref_sha}) != sha in \
                             FETCH_HEAD ({fh_sha}): il ref e' stato riposizionato dopo l'ultima \
                             fetch provata. Esegui `git fetch upstream`."
                        ));
                    }
                }
                match canonical {
                    None => {
                        nfail += 1;
                        if fail.len() < 5 {
                            fail.push(format!(
                                "ref refs/remotes/upstream/{name}: URL di provenienza in \
                                 FETCH_HEAD non interpretabile come host/owner/repo."
                            ));
                        }
                    }
                    Some(c) if c != expected_canonical => {
                        nfail += 1;
                        if fail.len() < 5 {
                            fail.push(format!(
                                "ref refs/remotes/upstream/{name}: provenienza {c} != upstream \
                                 atteso {expected_canonical}. `git remote set-url` cambia la \
                                 configurazione ma non i ref gia' scaricati, che restano del \
                                 remote precedente — l'URL oggi corretto non prova la provenienza \
                                 dei ref che quell'URL non ha mai scaricato. Esegui `git fetch \
                                 upstream` dal remote attualmente configurato."
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if fail.is_empty() {
        Ok(())
    } else {
        if nfail > fail.len() {
            fail.push(format!(
                "... ({nfail} ref totali senza provenienza provata)"
            ));
        }
        Err(fail)
    }
}

/// Un needle vietato col suo PERCHE' e il regime di case.
/// `ci = true`  => confronto case-insensitive (trailer/firme/marker: il
///                trailer git `co-authored-by` vale in ogni casing).
/// `ci = false` => case-sensitive (l'handle dell'operatore: il suo minuscolo
///                 non e' l'handle, vedi `op_handle`).
struct Vietato {
    needle: String,
    perche: &'static str,
    ci: bool,
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
            ci: true,
        },
        Vietato {
            needle: joined(&["Generated", " with"]),
            perche: "firma di generazione AI nel corpo del commit",
            ci: true,
        },
        Vietato {
            needle: String::from("\u{1F916}"),
            perche: "emoji del robot (attribuzione AI) nel corpo del commit",
            ci: true,
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
            ci: true,
        },
        Vietato {
            needle: joined(&["APPROVE", ":"]),
            perche: "marker di verdetto del register (audit interno) in note di release",
            ci: true,
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
            } else if v.ci {
                contains_ci(line, v.needle.as_str())
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

/// Ispezione della storia "nostra": i commit raggiungibili da HEAD ma non da
/// alcun ref `refs/remotes/upstream/*` (`HEAD --not <ref>`), con i corpi
/// scansionati per attribuzione AI. Log in una sola invocazione:
/// `%H%x00%B%x00` separa hash e corpo con NUL (il corpo e' NUL-free, lo split
/// e' robusto); `--not` applica ogni ref upstream come limite negativo
/// (esclusione ancestry): un commit raggiungibile da HEAD e da qualsiasi ref
/// upstream e' upstream, non nostro. Estratta in funzione perche' il controllo
/// negativo deve esercitare la STESSA ispezione su un fixture, non una copia.
fn ispeziona_storia(root: &Path, upstream_refs: &[String]) -> (usize, Vec<String>) {
    let mut log_args: Vec<&str> = vec!["log", "HEAD", "--not"];
    for r in upstream_refs {
        log_args.push(r.as_str());
    }
    log_args.push("--format=%H%x00%B%x00");
    let raw = git(root, &log_args);

    let parts: Vec<&str> = raw.split('\0').collect();
    let needles = ai_vietati();
    let mut colpe: Vec<String> = Vec::new();
    let mut visti = 0usize;
    let mut i = 0;
    while i + 1 < parts.len() {
        let sha = parts[i].trim();
        let body = parts[i + 1];
        i += 2;
        if sha.is_empty() {
            continue;
        }
        visti += 1;
        // I needle di attribuzione AI sono case-insensitive (ci = true): un
        // trailer `co-authored-by` in minuscolo morde quanto `Co-Authored-By`.
        let hit: Vec<&str> = needles
            .iter()
            .filter(|n| {
                if n.ci {
                    contains_ci(body, n.needle.as_str())
                } else {
                    body.contains(n.needle.as_str())
                }
            })
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
    (visti, colpe)
}

// ---- CLASSE 1: attribuzione AI nella STORIA del fork ----------------------

#[test]
fn classe1_attribuzione_ai_assente_dalla_storia_del_fork() {
    let root = repo_root();

    // AUTENTICAZIONE DEL REMOTE prima di fidarsene. Il gate esclude i commit
    // raggiungibili da refs/remotes/upstream/*: se quel remote punta altrove
    // (un fork, un mirror, un repo sbagliato), l'insieme dei commit "nostri" e'
    // SBAGLIATO — non vuoto, ma misurato contro il riferimento sbagliato.
    // Superata la guardia sul vuoto, il confronto avverrebbe contro il remote
    // sbagliato e un commit con attribuzione AI resterebbe nascosto (presente
    // nel remote sbagliato => classificato "upstream" => non ispezionato). L'URL
    // del remote deve corrispondere al repository upstream atteso: il criterio
    // e' la costante UPSTREAM_HOST/UPSTREAM_REPO, scritta qui nel test, non
    // assunta dall'ambiente. Se non corrisponde, il gate FALLISCE dicendo
    // perche' — non prova a indovinare, non degrada silenziosamente. Questo
    // precede la raccolta dei ref: l'autenticazione avviene prima che un
    // qualsiasi ref venga consultato, quindi lo stato di fetch e' irrilevante
    // per il gate fissato (un remote sbagliato cade all'URL, non ai ref).
    let upstream_url = match Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["remote", "get-url", "upstream"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => panic!(
            "il remote 'upstream' non esiste o 'git remote get-url' e' fallito (status {}): {}. \
             Il criterio 'nostro vs upstream' non e' applicabile senza un remote upstream \
             autenticato. Il gate non passa verde per assenza di ispezione.",
            o.status,
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => panic!("git non eseguibile: {e}"),
    };
    let upstream_url = upstream_url.trim();
    let canonical = canonical_remote(upstream_url).unwrap_or_else(|| {
        panic!(
            "URL del remote 'upstream' non interpretabile come host/owner/repo: \
             {upstream_url:?}. Il gate non degrada su un remote di forma ignota: un \
             URL non riconoscibile e' un fail, non un silenzio."
        );
    });
    let expected = format!("{}/{}", UPSTREAM_HOST, UPSTREAM_REPO);
    assert!(
        canonical == expected,
        "il remote 'upstream' non e' il repository upstream atteso: trovato {canonical:?} \
         (URL {upstream_url:?}), atteso {expected:?}. Un remote che punta altrove — un fork, \
         un mirror, un repo sbagliato — rende l'insieme dei commit 'nostri' SBAGLIATO: superata \
         la guardia sul vuoto, il confronto avviene contro il riferimento sbagliato e un commit \
         con attribuzione AI resta nascosto (presente nel remote sbagliato => classificato \
         'upstream' => non ispezionato). Correggi il remote con `git remote set-url upstream \
         <URL atteso>`. Il gate non prova a indovinare e non degrada silenziosamente."
    );

    // CRITERIO (dichiarato): un commit e' "nostro" se e' raggiungibile da
    // HEAD ma NON da alcun ref remotizzato di upstream
    // (refs/remotes/upstream/*), non solo da upstream/main. Usa l'ancestry di
    // git — verita' di terra, non euristiche su autore o data.
    //
    // PERCHE' TUTTI I REF E NON SOLO upstream/main. I commit di release
    // upstream fuori dal ramo principale — backport su rami release/0.144,
    // [0.146], tag di release che vivono su rami di manutenzione — entrano
    // nella storia del fork via merge ma NON stanno su upstream/main.
    // Escluderli solo via upstream/main li attribuirebbe a noi: falso
    // positivo, il guardiano accusa l'innocente. Escludere TUTTI i ref
    // remotizzati di upstream restituisce a chi di diritto i commit di
    // release che stanno su un ramo remotizzato (release/0.144, 0.146, …).
    // Non copre, e non promette di coprire, i commit di release raggiungibili
    // SOLO da un tag, senza alcun ramo remotizzato che li contenga: quelli
    // restano classificati "nostro" — falso positivo dichiarato, vedi RESIDUO
    // sotto. Dire "li restituisce tutti a chi di diritto" prometterebbe piu'
    // di cio' che il codice mantiene: il gate non restituisce ogni commit di
    // release a chi di diritto, restituisce quelli sui rami remotizzati e
    // flagga (non nasconde) gli altri.
    //
    // Questo non puo' mai escludere un commit NOSTRO: non pubblichiamo su
    // upstream (siamo un fork downstream), quindi un nostro commit non e'
    // raggiungibile da refs/remotes/upstream/*. L'allargamento rimuove solo
    // falsi positivi, non introduce falsi negativi.
    //
    // RESIDUO DICHIARATO (verso d'errore scelto): un commit upstream
    // raggiungibile SOLO da un tag — non da alcun ramo remotizzato — resterebbe
    // classificato "nostro". E' un FALSO POSITIVO: il gate si arrossa,
    // l'operatore verifica, vede che e' upstream, lo smista. Si sceglie questo
    // verso (falso positivo che chiede verifica umana) invece del falso
    // negativo (un commit nostro escluso e una traccia che esce): il gate
    // esiste per non far uscire tracce, e un falso positivo si chiarisce a
    // mente, un falso negativo non si chiarisce mai. I tag non si escludono
    // proprio per non allargare il verso sbagliato: distinguere un tag upstream
    // da uno nostro e' fragile, e in dubbio si flagga.
    //
    // Perche' non autore: il merge commit che porta upstream e' comunque
    // nostro (autore del fork), e upstream contribuisce con email eterogenee
    // — l'autore e' un proxy, non la verita'. Perche' non data: la data del
    // fork non e' un confine netto (mergiamo upstream continuamente).
    // L'ancestry lo e': un commit o sta nella storia pubblicata da upstream
    // (qualsiasi suo ramo remotizzato), o e' nostro.
    //
    // Dipendenza: almeno un ref refs/remotes/upstream/* deve esistere
    // (`git fetch upstream`). Se non c'e' neanche uno, il gate FALLISCE: il
    // criterio non e' applicabile e non si passa verde per assenza di ispezione.
    let upstream_refs: Vec<String> = git(
        &root,
        &["for-each-ref", "--format=%(refname)", "refs/remotes/upstream"],
    )
    .lines()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
    assert!(
        !upstream_refs.is_empty(),
        "nessun ref refs/remotes/upstream/* presente: il criterio 'nostro vs upstream' \
         non e' applicabile. Esegui `git fetch upstream`. Il gate non passa verde \
         per assenza di ispezione."
    );

    // PROVENIENZA DEI REF (dati), non solo dell'URL (config). L'autenticazione
    // dell'URL sopra dimostra solo che la CONFIGURAZIONE punta al remote atteso:
    // NON dimostra che i ref `refs/remotes/upstream/*` siano stati scaricati DA
    // quel remote. `git remote set-url` cambia la configurazione ma NON
    // ripulisce i ref gia' scaricati, che restano del remote precedente: l'URL
    // oggi corretto non prova la provenienza dei ref che quell'URL non ha mai
    // scaricato. Un ref stale puo' rendere raggiungibile un commit vietato da
    // `refs/remotes/upstream/*` => classificato "upstream" => non ispezionato =>
    // NASCOSTO (misurato: un commit selezionato, zero colpe). Il gate si fida
    // dei ref solo se puo' provarne la provenienza (FETCH_HEAD: stesso url
    // atteso + stesso sha per ogni ref); se non puo', CADE dicendolo — non
    // indovina, non degrada. Prova locale, no rete: l'operatore soddisfa con
    // `git fetch upstream`.
    let expected_canonical = format!("{}/{}", UPSTREAM_HOST, UPSTREAM_REPO);
    if let Err(prov) = provenienza_refs_provata(&root, &expected_canonical) {
        panic!(
            "la provenienza dei ref refs/remotes/upstream/* NON e' provata: il gate non si fida \
             di ref la cui origine non puo' dimostrare. L'URL del remote (config) e' quello atteso \
             ma i ref non risultano scaricati da esso — `git remote set-url` cambia la \
             configurazione, non i ref gia' scaricati, che restano del remote precedente. Un ref \
             stale puo' nascondere un commit vietato (classificato 'upstream' => non ispezionato): \
             un commit selezionato e zero colpe. Prova locale, no rete: ogni ref deve essere in \
             FETCH_HEAD, dal remote atteso, con lo stesso sha. L'operatore soddisfa la condizione \
             con `git fetch upstream` (fetch piena dal remote attualmente configurato).\n  {}",
            prov.join("\n  ")
        );
    }

    // Ispezione della storia "nostra" (HEAD --not <refs/remotes/upstream/*>):
    // log --format NUL-separated, scan dei needle di attribuzione AI. La
    // logica e' in `ispeziona_storia` per condividerla con il controllo negativo,
    // che deve esercitare la STESSA ispezione su un fixture.
    let (visti, colpe) = ispeziona_storia(&root, &upstream_refs);

    // DIFETTO 1 — il gate non passa verde perche' l'insieme e' vuoto. Questo e'
    // il punto piu' insidioso: un test che itera su niente passa sempre, e un
    // gate anti-verde-vuoto che diventa verde-vuoto e' il fallimento piu'
    // istruttivo. Un insieme vuoto di commit "nostri" NON e' successo: e'
    // l'impossibilita di misurare, e va detta.
    //
    // Cause: (1) un ref upstream e' andato oltre HEAD (ref stallo / fetch non
    //   aggiornato) — HEAD e' antenato di un ref upstream, quindi
    //   `HEAD --not <upstream>` seleziona zero commit;
    // (2) HEAD non ha diverguto da upstream, cioe' non ci sono commit nostri
    //   da sanificare — situazione legittima ma che il gate non puo' assumere;
    // (3) il selettore non seleziona (ref sbagliati, refspaces mutati).
    // In tutti i casi il gate non ha guardato nulla: lo dichiara e fallisce.
    assert!(
        visti > 0,
        "l'insieme dei commit da ispezionare (HEAD --not <refs/remotes/upstream/*>) \
         e' vuoto: il gate ha iterato su nulla e non ha guardato nessun commit. \
         Non e' un successo, e' l'impossibilita di misurare. Cause possibili: \
         (1) un ref upstream e' andato oltre HEAD (ref stallo): HEAD e' antenato \
         di un ref upstream e `HEAD --not <upstream>` seleziona zero commit — \
         esegui `git fetch upstream` e verifica; (2) HEAD non ha diverguto da \
         upstream, nessun commit nostro da sanificare; (3) il selettore non \
         seleziona (refspaces mutati). Un insieme vuoto va detto, non passato."
    );

    assert!(
        colpe.is_empty(),
        "la storia del fork (HEAD --not <refs/remotes/upstream/*>) non deve contenere \
         attribuzione AI:\n  {}",
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
    // handle resta CASE-SENSITIVE: "dag" minuscolo (directed acyclic graph,
    // struttura dati comune in codice reale) NON e' l'handle dell'operatore e
    // non deve mordere. Questa e' l'asimmetria voluta dal difetto 2: i
    // trailer/firme diventano case-insensitive, l'handle no — altrimenti il
    // guardiano griderebbe al lupo su ogni grafo di dipendenze.
    assert!(
        !has_isolated("let dag = build_dag();", &handle),
        "l'handle e' case-sensitive: 'dag' minuscolo non e' un leak"
    );

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
    // attribuzione AI: i needle mordono anche in casing NON canonico. Un
    // trailer scritto in minuscolo da un tool diverso (`co-authored-by`) o in
    // maiuscolo (`CO-AUTHORED-BY`, `GENERATED WITH`) deve cadere nel gate:
    // e' proprio il caso che il gate case-sensitive lasciava passare.
    assert!(
        contains_ci("co-authored-by: any assistant <x@y>", ai[0].needle.as_str()),
        "co-authored-by (minuscolo) deve mordere: confronto case-insensitive"
    );
    assert!(
        contains_ci("CO-AUTHORED-BY: x", ai[0].needle.as_str()),
        "CO-AUTHORED-BY (maiuscolo) deve mordere"
    );
    assert!(
        contains_ci("GENERATED WITH a tool", ai[1].needle.as_str()),
        "GENERATED WITH (maiuscolo) deve mordere"
    );

    // marker di audit: tutti i needle mordono in una riga di release.
    let am = audit_vietati();
    let bad_release = format!("release: {} {}verdict:codex-vl-x\n", am[0].needle, am[1].needle);
    assert!(
        am.iter().all(|v| bad_release.contains(v.needle.as_str())),
        "ogni needle di audit deve mordere"
    );
    // marker di audit: mordono anche in casing diverso (marker scritti a mano
    // maiuscolo/minuscolo).
    assert!(
        contains_ci("merge-FEATURE-register:verdetto", am[0].needle.as_str()),
        "merge-feature-register in casing misto deve mordere"
    );
    assert!(
        contains_ci("verdetto approve: ok", am[1].needle.as_str()),
        "approve: (minuscolo) deve mordere"
    );

    // Autenticazione del remote: il criterio morde su un remote sbagliato e
    // NON morde su quello giusto (entrambi i protocolli). Un remote che punta
    // al nostro fork invece che a upstream non deve passare per atteso.
    let expected = format!("{}/{}", UPSTREAM_HOST, UPSTREAM_REPO);
    for ok in [
        "https://github.com/openai/codex.git",
        "https://github.com/openai/codex",
        "git@github.com:openai/codex.git",
        "ssh://git@github.com/openai/codex.git",
    ] {
        let c = canonical_remote(ok).expect("URL legittimo non interpretato");
        assert!(
            c == expected,
            "remote legittimo non riconosciuto come atteso: {ok:?} -> {c:?} (atteso {expected:?})"
        );
    }
    // un remote che punta altrove (il nostro fork, un mirror, un repo vicino
    // ma non quello) NON e' upstream: il criterio lo distingue, non lo accetta
    // per vicinanza di host o di nome.
    for wrong in [
        "https://github.com/DioNanos/codex-vl.git",
        "https://github.com/openai/codex-cli.git",
        "git@github.com:forks/codex-mirror.git",
    ] {
        let c = canonical_remote(wrong).expect("URL sbagliato interpretato");
        assert!(
            c != expected,
            "un remote sbagliato non deve passare per atteso: {wrong:?} -> {c:?} == {expected:?}"
        );
    }
    // un URL non interpretabile e' None: fail, non degrade silenzioso.
    assert!(
        canonical_remote("non-e-un-url").is_none(),
        "un URL non interpretabile deve dare None, non un canonical fittizio"
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
        ci: false,
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

// ---- CONTROLLO NEGATIVO: ref stale + URL corretto => il gate CADE ---------

/// Rimuove directory scratch alla fine del test (anche su panic): il fixture
/// e' completamente autocontenuto, il repo reale non viene toccato e nessuno
/// stato resta in giro ("disattiva e ripristina nello stesso comando").
struct Scratch {
    dirs: Vec<PathBuf>,
}
impl Scratch {
    fn new(dirs: Vec<PathBuf>) -> Self {
        Scratch { dirs }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        for d in &self.dirs {
            let _ = fs::remove_dir_all(d);
        }
    }
}

#[test]
fn ref_stale_con_url_corretto_cade_invece_di_dare_zero_colpe() {
    // Caso misurato: ref stale (scaricati da un remote diverso) + URL corretto
    // (`git remote set-url` a openai/codex SENZA re-fetch). Senza il controllo
    // di provenienza il gate seleziona 1 commit, trova 0 colpe e VERDE: il
    // commit vietato resta nascosto (raggiungibile dal ref stale => classificato
    // "upstream" => non ispezionato). Con il controllo di provenienza il gate
    // CADE dicendo che i ref non sono provati. Rete: NESSUNA — i remote sono
    // file:// locali e l'URL corretto non viene mai fetchato; un gate che
    // facesse lui il fetch dipenderebbe dalla rete e cadrebbe a caso.

    let pid = std::process::id();
    let tmp = std::env::temp_dir().join(format!("fork-gate-stale-{pid}"));
    let wdir = std::env::temp_dir().join(format!("fork-gate-stale-w-{pid}"));
    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&wdir);
    let _scratch = Scratch::new(vec![tmp.clone(), wdir.clone()]);
    fs::create_dir_all(&tmp).expect("creazione dir scratch");

    let tmp_str = tmp.display().to_string();
    let wdir_str = wdir.display().to_string();

    // Repo fork: base A, commit vietato B (attribuzione AI), commit pulito C.
    git(&tmp, &["init", "-q", "-b", "main", "."]);
    git(&tmp, &["config", "user.name", "Gate Test"]);
    git(&tmp, &["config", "user.email", "gate@test.local"]);
    git(&tmp, &["config", "commit.gpgsign", "false"]);
    git(&tmp, &["commit", "--allow-empty", "-q", "-m", "base"]);
    // Commit vietato: attribuzione AI nel corpo (needle costruito a frammenti,
    // non letterale: il guardiano non si auto-accusa).
    let body_vietato = format!("{}: Bot <noreply@bot>", ai_vietati()[0].needle);
    git(
        &tmp,
        &[
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "feat: lavoro vietato",
            "-m",
            body_vietato.as_str(),
        ],
    );
    // B e' ora HEAD. Un bare clone qui fissa W.main = B (il commit vietato):
    // sara' il "remote upstream" SBAGLIATO da cui vengono i ref stale.
    let upstream_wrong = format!("file://{wdir_str}");
    git(
        &std::env::temp_dir(),
        &["clone", "-q", "--bare", tmp_str.as_str(), wdir_str.as_str()],
    );
    let b_sha = git(&tmp, &["rev-parse", "HEAD"]).trim().to_string();
    // Commit pulito C: HEAD avanza oltre B. Sara' l'unico commit "nostro"
    // selezionato (B resta raggiungibile dal ref stale => escluso => nascosto).
    git(
        &tmp,
        &["commit", "--allow-empty", "-q", "-m", "chore: pulito"],
    );

    // Remote upstream -> W (SBAGLIATO), fetch locale (file://, no rete).
    git(
        &tmp,
        &["remote", "add", "upstream", upstream_wrong.as_str()],
    );
    git(&tmp, &["fetch", "-q", "upstream"]);
    // Una plain fetch non crea refs/remotes/upstream/HEAD (non nel refspec
    // refs/heads/*:refs/remotes/upstream/*); se per varianti di git ci fosse,
    // lo si toglie per non confondere la provenienza con un ref non coperto da
    // FETCH_HEAD per ragioni estranee al difetto.
    let _ = git_try(&tmp, &["update-ref", "-d", "refs/remotes/upstream/HEAD"]);
    // set-url all'URL CORRETTO, SENZA re-fetch: la configurazione ora dice
    // openai/codex, ma i ref (e FETCH_HEAD) restano del remote precedente.
    git(
        &tmp,
        &[
            "remote",
            "set-url",
            "upstream",
            "https://github.com/openai/codex.git",
        ],
    );

    // Ref usati dal gate (solo main, in questo fixture).
    let urefs: Vec<String> = git(
        &tmp,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/remotes/upstream",
        ],
    )
    .lines()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();

    // (A) Il commit vietato B e' davvero nella storia di HEAD: non e' assente,
    // e' nascosto. E raggiungibile dal ref stale (W.main = B) => classificato
    // "upstream" => escluso dall'ispezione.
    assert!(
        git_try(
            &tmp,
            &["merge-base", "--is-ancestor", b_sha.as_str(), "HEAD"]
        )
        .is_some(),
        "il commit vietato deve essere nella storia di HEAD"
    );
    assert!(
        git_try(
            &tmp,
            &[
                "merge-base",
                "--is-ancestor",
                b_sha.as_str(),
                "refs/remotes/upstream/main"
            ]
        )
        .is_some(),
        "il commit vietato deve essere raggiungibile dal ref stale (nascosto)"
    );

    // (B) Senza controllo di provenienza: il gate seleziona 1 commit (C, pulito)
    // e trova ZERO colpe — il commit vietato e' nascosto. Questo e' il caso
    // misurato: "un commit selezionato e zero colpe trovate".
    let (visti, colpe) = ispeziona_storia(&tmp, &urefs);
    assert_eq!(
        visti, 1,
        "il gate buggy seleziona esattamente 1 commit (il pulito)"
    );
    assert!(
        colpe.is_empty(),
        "il gate buggy trova ZERO colpe: il vietato e' nascosto dal ref stale, ma le colpe sono: {colpe:?}"
    );

    // (C) Con il controllo di provenienza: URL corretto ma ref stale => la
    // provenienza NON e' provata (FETCH_HEAD registra il remote precedente, non
    // openai/codex) => il gate CADE invece di produrre zero colpe.
    let expected = format!("{}/{}", UPSTREAM_HOST, UPSTREAM_REPO);
    let prov = provenienza_refs_provata(&tmp, &expected);
    assert!(
        prov.is_err(),
        "con URL corretto ma ref stale la provenienza NON e' provata: il gate deve CADRE, non dare \
         zero colpe"
    );
    let msg = prov.unwrap_err().join(" ");
    assert!(
        msg.contains(&expected),
        "il gate dice quale remote attende e come soddisfare la condizione: {msg}"
    );
}
