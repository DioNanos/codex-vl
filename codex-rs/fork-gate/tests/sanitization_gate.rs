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

/// Radice del repo: cercata a RUNTIME, non cotta nel binario alla compilazione.
///
/// Il difetto che chiude: la radice era calcolata da `env!("CARGO_MANIFEST_DIR")`
/// — macro di COMPILAZIONE, il cui valore resta dentro il binario di test. Un
/// binario riusato dalla cache dopo che il worktree di build era stato rimosso
/// cercava la radice in un path morto: il gate era verde o rosso a seconda di
/// DOVE era stato compilato, non di cosa conteneva l'albero (misurato: tre test
/// panicavano col path di un worktree cancellato, e solo `cargo clean -p
/// fork-gate` restituiva la verita').
///
/// Le fonti, in ordine, sono tutte RUNTIME:
/// 1. la variabile d'ambiente CARGO_MANIFEST_DIR LETTA A RUNTIME: sotto
///    `cargo test` cargo la imposta per il processo di test a OGNI esecuzione,
///    col path del workspace vivo;
/// 2. la cwd: per l'esecuzione manuale del binario, da qualunque punto del
///    repo si parta.
/// Si risale finche' esiste un `.git` — file nei worktree, directory nel
/// checkout pieno: `.exists()` copre entrambi. Se nessuna fonte porta a una
/// radice il gate NON indovina: fallisce dicendo cosa ha provato, perche' puo'
/// succedere e come rimediare (`msg_radice_non_trovata`).
fn repo_root() -> PathBuf {
    let mut fonti: Vec<PathBuf> = Vec::new();
    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        if !m.is_empty() {
            fonti.push(PathBuf::from(m));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        fonti.push(cwd);
    }
    for f in &fonti {
        if let Some(root) = radice_da(f) {
            return root;
        }
    }
    panic!("{}", msg_radice_non_trovata(&fonti));
}

/// Risale da `start` finche' esiste `.git`. Pura: nessuna ipotesi, o la radice
/// con un `.git` VERO o None.
fn radice_da(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Il fallimento PARLANTE: se il gate non sa dove guardare, lo DICE — fonti
/// provate, causa piu' probabile, rimedio. Un panic ambiguo in questa posizione
/// si legge come un difetto di sanificazione e innesca la caccia al leak
/// sbagliata; uno che spiega se stesso si risolve in un minuto.
fn msg_radice_non_trovata(fonti: &[PathBuf]) -> String {
    let elenco = fonti
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "radice del repo non trovata (nessun .git risalendo da: {elenco}). \
         Il gate guarda l'albero da cui viene ESEGUITO, non quello in cui e' stato \
         compilato: eseguilo da dentro il repo (o con CARGO_MANIFEST_DIR che punti al \
         workspace vivo). Se il binario proviene dalla cache di build di un worktree \
         rimosso, `cargo clean -p fork-gate` forza la ricompilazione dal repo corrente. \
         Non e' un difetto di sanificazione: il gate non sa dove guardare e lo dichiara, \
         invece di morire in un punto ambiguo."
    )
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
/// (lowercase, senza suffisso .git). Riconosce SOLO le forme HTTPS
/// (https://host/owner/repo[.git]) e SSH con scheme (ssh://git@host/owner/repo)
/// e scp-like (git@host:owner/repo). Tutto il resto viene RIFIUTATO (None).
///
/// Questo e' un confine di sicurezza, non un normalizzatore tollerante: un
/// parser permissivo qui fabbrica una prova falsa invece di nessuna prova. Il
/// difetto che chiude: un URL locale come
/// `file:///tmp/source@github.com/openai/codex.git` si canonizzava come
/// `github.com/openai/codex` — scheme qualunque accettato e '@' raccolto
/// ovunque (rfind su tutta la stringa, anche nel path) — per cui sia la
/// configurazione sia FETCH_HEAD passavano per il repository atteso e un
/// commit vietato restava nascosto. Regole:
/// - scheme ammessi elencati (https, ssh); ogni altro scheme (file, git, http,
///   ...) e' None;
/// - '@' interpretato solo dove ha significato di userinfo: nell'autorita'
///   (fra "://" e il primo '/'), non nel path; nella forma scp-like, solo se
///   non c'e' un '/' prima del ':' (un '/' prima del ':' significa che il ':'
///   sta nel path, non e' scp-like);
/// - host preso per intero e confrontato per intero dai chiamanti (== / !=):
///   un lookalike come github.com.altro.example non coincide.
/// In dubbio si rifiuta (None), non si normalizza: un falso positivo costa una
/// verifica umana, un falso negativo fa uscire una traccia.
fn canonical_remote(url: &str) -> Option<String> {
    let u = url.trim();

    // scheme://... : SOLO https e ssh. L'autorita' e' la porzione fra "://" e
    // il primo dei caratteri '/', '?' o '#' (RFC 3986 §3.2: l'autorita'
    // termina al primo di path / query / fragment). Solo DENTRO l'autorita'
    // cosi' delimitata l' '@' ha significato di userinfo (user@host); un '@'
    // che cade nel path, nella query o nel fragment non e' userinfo.
    //
    // Il difetto che questo chiude: delimitare l'autorita' solo sul '/' faceva
    // considerare authority tutta la porzione fino al primo '/', inclusi eventuali
    // '?' e '#'. Cosi' `https://127.0.0.1:9?@github.com/openai/codex.git` — dove
    // l'autorita' reale e' `127.0.0.1:9` e `@github.com` e' QUERY — veniva
    // canonizzato come `github.com/openai/codex` (l' '@' dopo '?' raccolto come
    // userinfo): il gate si fidava di un remote che Git contatta altrove. Lo
    // stesso con '#' al posto di '?'. Ora l'autorita' termina al primo di '/?#
    // e l' '@' e' cercato solo dentro essa.
    if let Some(idx) = u.find("://") {
        // scheme CASE-SENSITIVE: non e' RFC 3986 (che lo vorrebbe
        // case-insensitive), e' il vero trasporto Git. Misurato (nessuna
        // connessione: fallisce PRIMA di ogni I/O): `git ls-remote
        // 'HTTPS://127.0.0.1:1/x'` -> "git: 'remote-HTTPS' is not a git
        // command" — Git cerca un remote helper esterno chiamato
        // git-remote-<scheme ESATTO, case preservato>, e solo git-remote-
        // https/git-remote-ssh minuscoli sono built-in. Con lo scheme
        // minuscolo verso lo stesso indirizzo, Git TENTA davvero la
        // connessione ("Failed to connect", non "is not a git command"):
        // il case conta per il dispatch, non e' un dettaglio cosmetico.
        // Un canonical_remote che lowercased lo scheme accettava
        // "HTTPS://github.com/..." come repository atteso — un URL che
        // Git stesso rifiuta di usare per QUALSIASI fetch reale, prima di
        // contattare alcunche'.
        let scheme = u.get(..idx)?;
        if scheme != "https" && scheme != "ssh" {
            return None;
        }
        let after = &u[idx + 3..];
        if after.is_empty() {
            return None;
        }
        let auth_end = after
            .find(|c| c == '/' || c == '?' || c == '#')
            .unwrap_or(after.len());
        let (auth, path) = after.split_at(auth_end);
        // https con PIU' di un '@' nell'autorita': Git (curl) tratta il
        // PRIMO '@' come separatore userinfo/host e lascia tutto il resto —
        // ogni '@' aggiuntivo incluso — come stringa host. Quella stringa
        // contiene '@', non e' un hostname valido, e curl rifiuta la
        // richiesta con "Bad hostname" PRIMA di qualunque connessione: non
        // esiste un host DNS-risolvibile diverso da quello atteso che Git
        // contatterebbe davvero in questo caso — solo un fallimento sempre e
        // comunque. Misurato (git credential fill e git ls-remote, nessun
        // dato sensibile in gioco): "https://a@b@github.com/..." -> Git
        // prova ad accedere a "https://b@github.com/..." e fallisce "URL
        // rejected: Bad hostname" — mai su github.com. Leggere qui l'ULTIMO
        // '@' (rfind) come faceva prima produceva "github.com": un host che
        // il parser dichiarava atteso mentre Git non l'avrebbe MAI
        // contattato. Per ssh:// e scp-like il discorso e' diverso: Git
        // passa la stringa intera a ssh senza parsarla lui stesso, ed e' SSH
        // (verificato con `ssh -G`) a usare l'ULTIMO '@' — li' rfind resta
        // corretto, per questo il controllo qui e' ristretto a https.
        if scheme == "https" && auth.matches('@').count() > 1 {
            return None;
        }
        // host: dopo l'ultimo '@' DENTRO l'autorita', prima di ':' (porta).
        let host = match auth.rfind('@') {
            Some(a) => &auth[a + 1..],
            None => auth,
        };
        let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
        if host.is_empty() {
            return None;
        }
        // tail: path SENZA query/fragment. Dopo l'autorita' ci puo' essere una
        // query ('?') o un fragment ('#') prima del path: le si scarta, il
        // canonical e' host/path — un URL con query/fragment che falsifica il
        // path atteso non deve passare per il repo atteso.
        let path = path.split_once('?').map(|(p, _)| p).unwrap_or(path);
        let path = path.split_once('#').map(|(p, _)| p).unwrap_or(path);
        let tail = path.trim_matches('/');
        // il suffisso ".git" va tolto dal PATH CANONICO, dopo aver scartato
        // query e fragment — non dall'URL grezzo intero. Il difetto che
        // questo chiude: lo strip avveniva su `url` prima ancora di
        // delimitare l'autorita' (`url.strip_suffix(".git")`), quindi
        // funzionava SOLO se ".git" era l'ultimo carattere dell'URL intero.
        // Con una query o un fragment in coda — "...codex.git?foo=bar",
        // "...codex.git#readme" — l'URL non finisce piu' per ".git": lo
        // strip non aveva effetto, ".git" restava nel path canonico
        // ("github.com/openai/codex.git" invece di
        // "github.com/openai/codex"), il confronto con l'atteso falliva, e
        // un remote PERFETTAMENTE ONESTO veniva rifiutato con un messaggio
        // che accusa "punta a un altro repo" — falso, e nel posto sbagliato:
        // il difetto era nel parser, non nel remote.
        let tail = tail.strip_suffix(".git").unwrap_or(tail);
        if tail.is_empty() {
            return None;
        }
        return Some(format!("{host}/{tail}").to_ascii_lowercase());
    }

    // forma scp-like: [user@]host:owner/repo. Qui l' '@' e' userinfo (forma
    // canonica scp) e il ':' separa host da path. Ma la forma e' valida SOLO
    // se non c'e' nessun '/' prima del ':': un '/' prima del ':' significa che
    // il ':' sta nel path (forma locale o altro), non e' scp-like. Senza questo
    // controllo, "tmp/source@github.com:owner/repo" — '@' in un prefisso di
    // path — si canonizzava come l'host atteso: la stessa decisione permissiva
    // del ramo scheme, fatta in un altro modo.
    if let Some(c) = u.find(':') {
        let before = &u[..c];
        if !before.contains('/') {
            let host = match before.rfind('@') {
                Some(a) => &before[a + 1..],
                None => before,
            };
            let tail = u[c + 1..].trim_end_matches('/');
            // stesso strip del ramo scheme:// sopra, per coerenza (qui non
            // c'e' query/fragment da scartare prima: SSH non ha quella
            // sintassi, l'intero resto dopo ':' e' path letterale).
            let tail = tail.strip_suffix(".git").unwrap_or(tail);
            if host.is_empty() || tail.is_empty() {
                return None;
            }
            return Some(format!("{host}/{tail}").to_ascii_lowercase());
        }
    }

    None
}

// ---- concordanza col comportamento REALE di Git ----------------------------
//
// La conformita' a RFC 3986 non e' la proprieta' che serve: e' un mezzo. La
// proprieta' che serve e': il parser deve vedere LO STESSO HOST che
// contatterebbe Git. Oggi coincidono per ogni URL provato, ma nessun test le
// legava — la conformita' RFC era verificata contro un'interpretazione
// scritta a mano dentro il test, non contro Git stesso. Le funzioni sotto
// chiedono a Git quale autorita' risolverebbe per un URL, SENZA MAI aprire
// una connessione di rete.
//
// https:// e ssh://: `git credential fill` fa il PARSING REALE che il
// trasporto userebbe per il lookup delle credenziali (stessa logica di
// http.c/connect.c) — non una ricostruzione a mano. Il credential.helper va
// SEMPRE azzerato esplicitamente (`-c credential.helper=`) PRIMA di
// aggiungerne uno fittizio: altrimenti si accoda a un helper REALE gia'
// configurato a livello di sistema/globale, che per un host reale (es.
// github.com) puo' restituire una credenziale VERA dell'operatore in chiaro
// sullo stdout del comando — misurato durante lo sviluppo di questo stesso
// test. L'helper fittizio risponde SEMPRE con user/pass finti, cosi' `git
// credential fill` non tenta mai I/O interattivo ne' di rete.
//
// scp-like ([user@]host:owner/repo): per SSH, Git passa la stringa
// "destination" COSI' COM'E' al comando ssh, senza interpretarla lui
// stesso. Si intercetta quel comando con GIT_SSH_COMMAND — uno script che
// stampa il suo primo argomento su STDERR e muore subito, prima di aprire
// un socket — per catturare la destination esatta che Git avrebbe usato,
// poi si chiede a `ssh -G <destination>` come SSH stesso la risolverebbe
// (hostname reale): di nuovo nessuna connessione, `-G` stampa la
// configurazione effettiva e basta.
//
// Limite dichiarato: per scp-like, se `destination` non e' un target SSH
// valido (es. contiene un carattere che OpenSSH stesso rifiuta nello
// username), `ssh -G` fallisce e la funzione ritorna None — non e' un'
// assenza di prova sull'host, e' l'assenza dell'oggetto da confrontare.
// Il test che usa queste funzioni tratta None come "nessuna concordanza da
// verificare", mai come "concordanza silenziosa".

/// Legge un campo (`host`, `path`, ...) dall'output di `git credential
/// fill` per `url`. None se Git stesso non riesce a interpretare l'URL.
fn credential_fill_field(url: &str, field: &str) -> Option<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .args(["-c", "credential.helper="])
        .args([
            "-c",
            "credential.helper=!f() { echo username=probe; echo password=probe; }; f",
        ])
        .args(["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child
        .stdin
        .take()?
        .write_all(format!("url={url}\n\n").as_bytes())
        .ok()?;
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("{field}=");
    text.lines()
        .find_map(|l| l.strip_prefix(prefix.as_str()).map(str::to_string))
}

/// Cattura la "destination" SSH che Git passerebbe DAVVERO al comando ssh
/// per `url` (scp-like o ssh://), senza mai aprire una connessione: il
/// comando ssh e' sostituito con uno script che stampa il suo primo
/// argomento su stderr e muore subito. `git ls-remote` fallisce SEMPRE (lo
/// script non implementa alcun protocollo git) — e' il fallimento atteso,
/// il dato utile e' nello stderr catturato prima di quel fallimento.
fn ssh_destination_for(url: &str) -> Option<String> {
    let output = Command::new("git")
        .env(
            "GIT_SSH_COMMAND",
            r#"sh -c 'echo "GATE_SSH_DEST:$1" >&2; exit 1' --"#,
        )
        .args(["ls-remote", url])
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .find_map(|l| l.strip_prefix("GATE_SSH_DEST:").map(str::to_string))
}

/// Hostname che SSH risolverebbe per `destination` (`[user@]host`),
/// interrogando la configurazione effettiva di SSH (`-G`) senza mai
/// connettersi. None se SSH stesso rifiuta la destination.
fn ssh_resolved_host(destination: &str) -> Option<String> {
    let output = Command::new("ssh")
        .args(["-G", destination])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|l| l.strip_prefix("hostname ").map(|h| h.trim().to_string()))
}

/// Rimuove una porta finale (":NNNN") da un host/autorita', se presente.
fn strip_port(host: &str) -> &str {
    host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host)
}

/// Autorita' (solo hostname, senza porta) che GIT contatterebbe DAVVERO per
/// `url`, secondo Git/SSH stessi. None se Git non ha un'opinione
/// utilizzabile: uno scheme che ne' il credential subsystem ne' SSH
/// coprono, o un URL che nessuno dei due sa interpretare affatto.
///
/// Riconsegna (due giri, non uno): un primo giro aveva reso questa
/// funzione case-INSENSITIVE sullo scheme (misurato con `git credential
/// fill` su "HTTPS://...": risponde host=github.com). Sembrava corretto —
/// ma quella misura guardava il CREDENTIAL SUBSYSTEM, non il vero
/// dispatch del trasporto. Verificato *anche* quello (nessuna rete: fallisce
/// PRIMA di ogni I/O): `git ls-remote 'HTTPS://127.0.0.1:1/x'` -> "git:
/// 'remote-HTTPS' is not a git command"; con lo scheme minuscolo verso lo
/// stesso indirizzo, Git TENTA davvero la connessione ("Failed to
/// connect"). Il vero trasporto di Git cerca un remote helper chiamato
/// `git-remote-<scheme ESATTO>`, e solo `git-remote-https`/`-ssh`
/// minuscoli sono built-in: uno scheme con case diverso non produce MAI
/// un fetch riuscito, qualunque cosa dica il credential subsystem (che
/// serve solo al lookup delle credenziali, non al dispatch del trasporto).
/// `credential fill` e il trasporto reale sono due sotto-sistemi diversi
/// di Git, e qui NON concordano fra loro. La proprieta' che questo file
/// deve misurare e' "l'host che Git contatterebbe per un fetch reale", non
/// "l'host che il credential subsystem crede di dover cercare" — quindi lo
/// scheme resta case-SENSITIVE qui, ed e' `canonical_remote` (sopra) che
/// e' stato corretto per allinearsi, non questa funzione.
fn git_authority_for(url: &str) -> Option<String> {
    if url.starts_with("https://") {
        return credential_fill_field(url, "host").map(|h| strip_port(&h).to_string());
    }
    if url.starts_with("ssh://") {
        let dest = ssh_destination_for(url)?;
        return ssh_resolved_host(&dest);
    }
    // forma scp-like: nessuno scheme ("https://"/"ssh://" gia' esclusi
    // sopra) — valida SOLO se non c'e' un '/' prima del ':' (altrimenti il
    // ':' e' nel path, non separa host) e l'URL non ha comunque un "://"
    // altrove (uno scheme sconosciuto o mal capitalizzato come "HTTPS://"
    // o "file://" non e' scp-like solo perche' contiene un ':'). Stessa
    // condizione di canonical_remote: non e' una coincidenza, e' l'unico
    // modo per cui la domanda "Git la tratterebbe
    // come SSH?" ha senso per questa forma.
    let colon = url.find(':')?;
    if url[..colon].contains('/') || url[colon..].starts_with("://") {
        return None;
    }
    let dest = ssh_destination_for(url)?;
    ssh_resolved_host(&dest)
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
/// FETCH_HEAD assente, detto PER INTERO. La causa sottile che merita il nome:
/// FETCH_HEAD e' PER-WORKTREE, i ref `refs/remotes/upstream/*` sono CONDIVISI
/// da tutti i checkout del repo. Un gate girato in un worktree dove nessuna
/// fetch e' mai stata fatta vede i ref (scaricati da un altro checkout) ma non
/// puo' provarne la provenienza — e un gate che non puo' provarla non passa
/// verde per assenza di ispezione. Il rimedio ha un POSTO preciso: la fetch va
/// fatta NEL checkout dove gira il gate, perche' quella fatta nel checkout
/// principale scrive il FETCH_HEAD del principale e questo resta senza prova
/// (misurato: gate rosso nel worktree nuovo, verde nel principale, stesso
/// repo). Non e' un difetto di sanificazione ne' nei ref: e' una condizione
/// locale mancante, e ora e' detta.
fn msg_fetch_head_assente(fh_path: &Path) -> String {
    format!(
        "FETCH_HEAD assente ({}): nessuna fetch e' mai stata registrata in QUESTO \
         checkout. FETCH_HEAD e' per-worktree mentre i ref refs/remotes/upstream/* sono \
         condivisi fra tutti i checkout del repo: i ref si vedono anche senza una fetch \
         locale, la loro PROVENIENZA no — e un gate che non puo' provarla non passa \
         verde per assenza di ispezione. Rimedio: `git fetch upstream` NEL checkout \
         dove gira il gate (farla nel checkout principale scrive il FETCH_HEAD del \
         principale e lascia questo senza prova). Non e' un difetto di sanificazione \
         ne' nei ref: e' una condizione locale mancante, detta invece di nascosta.",
        fh_path.display()
    )
}

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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(vec![msg_fetch_head_assente(&fh_path)]);
        }
        Err(e) => {
            return Err(vec![format!(
                "FETCH_HEAD presente ma non leggibile ({}): {e}. La provenienza dei ref \
                 non e' provata: il gate non prende la parola di un file che non riesce ad \
                 aprire. Verifica i permessi, o ricrea la fetch con `git fetch upstream`.",
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
                                 FETCH_HEAD non interpretabile come host/owner/repo (atteso \
                                 {expected_canonical}). Un URL non riconoscibile — scheme non \
                                 ammesso o forma ambigua — e' un fail, non un degrade: esegui \
                                 `git fetch upstream` dal remote attualmente configurato."
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

    // https con PIU' di un '@' nell'autorita': None, non un canonical
    // fittizio. Questo URL viveva prima nella lista "honest" con la
    // giustificazione "l'host e' dopo l'ULTIMO '@'" — ma quella e' una
    // regola scritta a mano nel test, non il comportamento di Git. Misurato
    // (git credential fill, nessuna connessione: `git -c credential.helper=
    // -c 'credential.helper=!f(){ echo username=x; echo password=x; };f'
    // credential fill` con url=... in input) e confermato con `git
    // ls-remote` (fallisce sempre, nessun dato sensibile in gioco perche'
    // l'hostname e' rifiutato prima di qualunque tentativo di rete): Git
    // tratta il PRIMO '@' come separatore userinfo/host, non l'ultimo — per
    // "https://a@b@github.com/..." risolve host="b@github.com" (userinfo
    // "a"), non "github.com". "b@github.com" non e' un hostname valido:
    // curl rifiuta con "URL rejected: Bad hostname" prima di qualunque
    // connessione. Non esiste quindi un host DNS-risolvibile diverso da
    // quello atteso che Git contatterebbe con questo URL — ma il vecchio
    // parser lo dichiarava "atteso" (github.com) quando Git non ci sarebbe
    // MAI andato: la proprieta' che conta ("il parser vede lo stesso host
    // che contatterebbe Git") era violata anche se l'esito di sicurezza
    // pratico restava innocuo. Vedi il test
    // `canonical_remote_concorda_con_git_sullautorita` per la verifica
    // sistematica di questa proprieta' su tutti i casi qui sopra.
    assert!(
        canonical_remote("https://a@b@github.com/openai/codex.git").is_none(),
        "https con piu' di un '@' nell'autorita' deve dare None: Git tratta \
         il primo '@' come separatore, non l'ultimo, e il resto contenente \
         '@' non e' un hostname valido (misurato: Bad hostname, mai github.com)"
    );

    // CONTROLLO NEGATIVO — il gate che guardava e veniva ingannato. Un parser
    // permissivo in un confine di sicurezza fabbrica una prova falsa invece di
    // nessuna prova: scheme qualunque + '@' raccolto ovunque (rfind su tutta la
    // stringa, anche nel path) faceva canonizzare un URL locale come il repo
    // atteso, per cui sia la configurazione sia FETCH_HEAD passavano per
    // github.com/openai/codex e un commit vietato restava nascosto. Ora cio'
    // che non e' inequivocabilmente il repository atteso viene RIFIUTATO.
    //
    // (1) L'URL ESATTO dell'audit: scheme file + '@' nel path. Misurato prima
    // della correzione: canonizzava come github.com/openai/codex (== atteso).
    assert!(
        canonical_remote("file:///tmp/source@github.com/openai/codex.git").is_none(),
        "un URL file:// con '@' nel path non deve canonizzarsi come upstream: scheme non ammesso"
    );
    // (2) '@' nel path SENZA scheme file: la stessa idea, via forma scp-like.
    // Un prefisso con '/' prima del ':' non e' scp-like (il ':' e' nel path).
    // Misurato prima: canonizzava come github.com/openai/codex.
    assert!(
        canonical_remote("tmp/source@github.com:openai/codex").is_none(),
        "un input con '@' in un prefisso di path e ':' dopo non e' scp-like: deve dare None"
    );
    // (3) host che CONTIENE quello atteso come sottostringa: l'host e' preso
    // per intero e confrontato per intero (== / !=), non per sottostringa — un
    // lookalike non coincide con github.com/openai/codex.
    let lookalike = canonical_remote("https://github.com.altro.example/openai/codex.git");
    assert!(
        lookalike.as_deref() != Some(expected.as_str()),
        "un host lookalike (github.com.altro.example) non deve passare per github.com: {:?} == {expected:?}",
        lookalike
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

// ---- CONTROLLO NEGATIVO: autorita' reale != attesa, l'URL ingannevole viene
// rifiutato comunque sia scritto. Pinna la proprieta' RFC 3986 che
// l'autenticazione del remote avrebbe dovuto chiudere ma non chiudeva:
// l'autorita' di un URL termina al primo di '/', '?' o '#' (RFC 3986 §3.2), e
// l' '@' di userinfo va cercato SOLO dentro l'autorita' cosi' delimitata.
// Delimitare solo sul '/' faceva canonizzare
// `https://127.0.0.1:9?@github.com/openai/codex.git` come
// `github.com/openai/codex` (l' '@' dopo '?' raccolto come userinfo): il gate
// si fidava di un remote che Git contatta a 127.0.0.1:9. Il discriminante non
// e' "il test fallisce" (classe1 e' gia' rosso di suo) ma il MOTIVO: un remote
// onesto e uno ingannevole producevano lo stesso panic (classe1, attribuzione
// AI), cioe' il gate si fidava di entrambi. Qui si verifica che il parser
// espone l'autorita' REALE — host diverso da github.com, o None — che e' il
// motivo per cui il gate di integrazione rifiuta invece di fidarsene.

/// Host di un canonical "host/owner/repo": la porzione prima del primo '/'.
fn canonical_host(c: &str) -> &str {
    c.split_once('/').map(|(h, _)| h).unwrap_or(c)
}

#[test]
fn url_ingannevole_autorita_reale_non_attesa_viene_rifiutato() {
    let expected = format!("{}/{}", UPSTREAM_HOST, UPSTREAM_REPO);

    // URL ingannevoli: l'autorita' reale NON e' github.com, ma l'URL e'
    // scritto per far sembrare che lo sia ( '@' nella query/fragment, host
    // atteso nel path, '@' con host finto prima di quello reale). Tutti
    // devono essere rifiutati come upstream, e il MOTIVO deve essere visibile:
    // l'host che il parser estrae non e' github.com.
    let deceptive: &[&str] = &[
        // '?@' : '@github.com' e' QUERY, l'autorita' reale e' 127.0.0.1:9.
        "https://127.0.0.1:9?@github.com/openai/codex.git",
        // '#@' : '@github.com' e' FRAGMENT, stessa idea via '#'.
        "https://127.0.0.1:9#@github.com/openai/codex.git",
        // '?' e '#' combinati.
        "https://127.0.0.1:9?#@github.com/openai/codex.git",
        "https://127.0.0.1:9#?@github.com/openai/codex.git",
        // '@' dopo '?' con un path reale prima della query: l'autorita' reale
        // 127.0.0.1 resta visibile nel canonical (motivo osservabile).
        "https://127.0.0.1:9/openai/codex?@github.com/openai/codex.git",
        // host atteso che compare nel PATH, non nell'autorita'.
        "https://evil.example.com/github.com/openai/codex.git",
        // confusione '@' classica: github.com come userinfo, host reale altrove.
        "https://github.com@evil.example/openai/codex.git",
    ];
    for url in deceptive {
        let got = canonical_remote(url);
        // Proprieta': non passa per il repo atteso.
        assert_ne!(
            got.as_deref(),
            Some(expected.as_str()),
            "URL ingannevole non deve passare per upstream atteso: {url:?} -> {got:?}"
        );
        // MOTIVO: l'host che il parser estrae (l'autorita' reale) non e'
        // github.com. Per None l'autorita' reale non lascia un path
        // interpretabile — comunque non github.com. Questo e' il punto: il
        // parser vede DENTRO l'URL, non si fida dell'apparenza.
        let host = got.as_deref().map(canonical_host);
        assert_ne!(
            host,
            Some(UPSTREAM_HOST),
            "il canonical deve mostrare l'autorita' reale, non l'host atteso: \
             {url:?} -> host {host:?}"
        );
    }

    // Una singola credenziale userinfo e URL onesti con query/fragment: NON
    // sono inganni. Una query/fragment dopo il path non cambia il repo. Il
    // gate non deve over-restringere e rifiutare un remote onesto: un falso
    // positivo costa una verifica umana, ma accusare l'innocente erode il
    // guardiano quanto fidarsi del colpevole.
    //
    // I quattro casi con ".git" PRIMA di query/fragment sono la forma
    // completa, non un caso in piu': un falso positivo trovato dall'audit
    // (riprodotto: vedi il fix dello strip ".git" in canonical_remote) —
    // "https://github.com/openai/codex.git?foo=bar" veniva RIFIUTATO,
    // accusato di puntare a un repo diverso ("...codex.git" != "...codex"),
    // mentre punta esattamente al repository atteso. Lo strip di ".git"
    // avveniva sull'URL grezzo intero, prima ancora di delimitare
    // l'autorita': con query o fragment in coda l'URL non finisce piu' per
    // ".git" e lo strip non aveva effetto. Questi casi erano stati tolti dal
    // test in una consegna precedente per evitare la combinazione invece di
    // correggere il parser — il test era stato piegato al difetto. Restano
    // qui nella forma completa.
    for honest in [
        "https://token@github.com/openai/codex.git",
        "https://github.com/openai/codex?foo=bar",
        "https://github.com/openai/codex#readme",
        "https://github.com/openai/codex.git?foo=bar",
        "https://github.com/openai/codex.git#readme",
        "https://github.com/openai/codex.git?foo=bar#readme",
    ] {
        let got = canonical_remote(honest).expect("URL onesto non interpretato");
        assert_eq!(
            got, expected,
            "URL onesto deve passare per atteso: {honest:?} -> {got:?} (atteso {expected:?})"
        );
    }
}

// ---- la proprieta' che conta: il parser vede lo stesso host di Git -------
//
// Non "canonical_remote e' conforme a RFC 3986" — quella e' un mezzo. La
// proprieta' e': quando canonical_remote() accetta un URL, l'host che
// estrae deve coincidere con l'host che GIT STESSO risolverebbe per quello
// stesso URL. Se canonical_remote rifiuta (None), va sempre bene: un
// rifiuto e' fail-closed per costruzione, non richiede che Git concordi
// sul rifiuto (Git potrebbe comunque "avere un'opinione" su un URL che il
// gate ha gia' scartato — non e' un problema, e' prudenza in piu').
//
// Questo test non inventa l'interpretazione di nessun URL: la chiede a Git
// (git_authority_for, sopra). E' rosso di suo se e solo se emerge una vera
// divergenza — non era mai successo finora nello sviluppo di questo file,
// ma la proprieta' era comunque non verificata: la conformita' RFC 3986
// veniva controllata contro un'attesa scritta a mano nel test, mai contro
// Git.
#[test]
fn canonical_remote_concorda_con_git_sullautorita() {
    let expected_host = UPSTREAM_HOST;

    // Ogni URL gia' esercitato altrove in questo file dai test sopra, piu'
    // il caso rifiutato per la divergenza trovata scrivendo QUESTO test
    // (vedi sotto): l'insieme e' lo stesso, la domanda e' diversa — non
    // "canonical_remote da' l'esito che il test si aspetta" ma "quando
    // canonical_remote accetta, Git concorda sull'host".
    let urls: &[&str] = &[
        // legittimi (tutte le forme)
        "https://github.com/openai/codex.git",
        "https://github.com/openai/codex",
        "git@github.com:openai/codex.git",
        "ssh://git@github.com/openai/codex.git",
        // sbagliati ma interpretabili (host reale, diverso da quello atteso)
        "https://github.com/DioNanos/codex-vl.git",
        "https://github.com/openai/codex-cli.git",
        "git@github.com:forks/codex-mirror.git",
        // non interpretabile
        "non-e-un-url",
        // rifiutati per scheme non ammesso o forma non scp-like
        "file:///tmp/source@github.com/openai/codex.git",
        "tmp/source@github.com:openai/codex",
        // lookalike host (sottostringa, non identita')
        "https://github.com.altro.example/openai/codex.git",
        // RFC 3986: autorita' delimitata da '/','?','#', '@' cercato solo
        // dentro l'autorita' cosi' delimitata
        "https://127.0.0.1:9?@github.com/openai/codex.git",
        "https://127.0.0.1:9#@github.com/openai/codex.git",
        "https://127.0.0.1:9?#@github.com/openai/codex.git",
        "https://127.0.0.1:9#?@github.com/openai/codex.git",
        "https://127.0.0.1:9/openai/codex?@github.com/openai/codex.git",
        "https://evil.example.com/github.com/openai/codex.git",
        "https://github.com@evil.example/openai/codex.git",
        // onesti con credenziale singola o query/fragment
        "https://token@github.com/openai/codex.git",
        "https://github.com/openai/codex?foo=bar",
        "https://github.com/openai/codex#readme",
        // divergenza REALE trovata dall'audit e riprodotta: lo strip di
        // ".git" operava sull'URL grezzo intero, non sul path canonico —
        // con query/fragment in coda ".git" restava nel path e questi URL
        // (perfettamente onesti: Git contatta ESATTAMENTE
        // github.com/openai/codex) venivano rifiutati, accusati di puntare
        // a un repo diverso. Qui e' il primo caso in cui parser e Git
        // divergevano davvero — non un'ipotesi caduta alla misura come il
        // caso backslash, un vero falso positivo. Vedi il fix dello strip
        // ".git" in canonical_remote.
        "https://github.com/openai/codex.git?foo=bar",
        "https://github.com/openai/codex.git#readme",
        "https://github.com/openai/codex.git?foo=bar#readme",
        // divergenza trovata scrivendo questo test (vedi sotto per la misura
        // e il fix): https con PIU' di un '@' nell'autorita'.
        "https://a@b@github.com/openai/codex.git",
    ];

    let mut checked = 0usize;
    let mut no_git_opinion = Vec::new();
    for url in urls {
        let parser_host = canonical_remote(url).map(|c| canonical_host(&c).to_string());
        let Some(parser_host) = parser_host else {
            // Rifiuto: sempre sicuro, non richiede concordanza da Git.
            continue;
        };
        match git_authority_for(url) {
            Some(git_host) => {
                checked += 1;
                assert_eq!(
                    parser_host, git_host,
                    "canonical_remote accetta {url:?} come host {parser_host:?}, \
                     ma Git risolverebbe {git_host:?}: il parser non vede lo \
                     stesso host che contatterebbe Git"
                );
            }
            None => no_git_opinion.push(*url),
        }
    }
    assert!(
        no_git_opinion.is_empty(),
        "canonical_remote accetta un URL su cui Git non ha nemmeno un'opinione \
         interrogabile (nessun oggetto da confrontare, non una concordanza): {no_git_opinion:?}"
    );
    assert!(
        checked >= 10,
        "il test deve aver confrontato un numero sostanziale di URL con Git, \
         non solo aver attraversato rifiuti: controllati {checked}"
    );

    // Il caso che sopra e' nella lista ma per costruzione (canonical_remote
    // rifiuta) non entra nel confronto host-per-host: verificarlo
    // esplicitamente e' il punto di questo test. MISURA (nessuna
    // connessione: git credential fill con helper azzerato; confermato con
    // git ls-remote reale, che fallisce sempre e non scambia mai dati
    // sensibili perche' l'hostname e' rifiutato prima di qualunque
    // connessione): per "https://a@b@github.com/openai/codex.git" Git
    // risolve host="b@github.com" (userinfo "a", separatore = PRIMO '@',
    // non l'ultimo) — un hostname che CONTIENE '@' e che curl rifiuta con
    // "URL rejected: Bad hostname" prima di qualunque tentativo di rete.
    // canonical_remote rifiuta (None): corretto, perche' Git non
    // contatterebbe MAI un host DNS-risolvibile per questo URL — ma il
    // vecchio parser (rfind, ultimo '@') leggeva "github.com" qui, una
    // proprieta' falsa anche se innocua in pratica (nessun host malevolo
    // realmente raggiungibile: qualunque autorita' https con 2+ '@' produce
    // sempre, dopo il primo '@', un resto che CONTIENE '@' e viene sempre
    // rifiutato da curl come hostname malformato — verificato anche con 3
    // '@' consecutivi).
    let deceptive_multi_at = "https://a@b@github.com/openai/codex.git";
    assert!(
        canonical_remote(deceptive_multi_at).is_none(),
        "https con piu' di un '@' deve restare rifiutato"
    );
    match git_authority_for(deceptive_multi_at) {
        Some(git_host) => assert_ne!(
            strip_port(&git_host),
            expected_host,
            "se Git avesse un'opinione su questo URL, non deve MAI essere \
             l'host atteso (altrimenti rifiutarlo sarebbe un falso negativo, \
             non prudenza): {git_host:?}"
        ),
        None => {
            // Anche se in questa esecuzione git_authority_for non riesce a
            // interrogare Git (es. `git credential fill` fallisse per un
            // motivo ambientale), il rifiuto di canonical_remote resta
            // corretto e verificato sopra: non e' un requisito che questo
            // ramo produca Some per essere una consegna valida.
        }
    }

    // Secondo giro sullo stesso test, stesso principio ("chiedi a Git, non
    // ricostruire"): un primo tentativo aveva reso l'oracolo case-
    // insensitive sullo scheme, misurando SOLO `git credential fill`
    // (che risponde a "HTTPS://..." con host=github.com). Verificato
    // *anche* il vero trasporto (nessuna rete: fallisce PRIMA di ogni I/O
    // in entrambi i casi): `git ls-remote 'HTTPS://127.0.0.1:1/x'` -> "git:
    // 'remote-HTTPS' is not a git command"; con lo scheme minuscolo verso
    // lo stesso indirizzo, Git TENTA davvero la connessione ("Failed to
    // connect"). Il credential subsystem e il dispatch del trasporto sono
    // due sotto-sistemi DIVERSI di Git e qui non concordano: "HTTPS://..."
    // non produce MAI un fetch riuscito, qualunque cosa dica il
    // credential lookup. La proprieta' che conta e' "l'host che Git
    // contatterebbe per un fetch reale" — quindi lo scheme resta
    // case-sensitive, ed e' canonical_remote che e' stato corretto per
    // allinearsi (era lui il permissivo di troppo, non l'oracolo).
    for mismatched_case in ["HTTPS://github.com/openai/codex.git", "SSH://git@github.com/openai/codex.git"] {
        assert!(
            canonical_remote(mismatched_case).is_none(),
            "uno scheme con case diverso da \"https\"/\"ssh\" non produce mai un \
             fetch riuscito (Git cerca un remote helper esterno case-preservato \
             che non esiste): {mismatched_case:?} deve restare rifiutato"
        );
        // L'oracolo deve concordare sul rifiuto: nessuna opinione
        // utilizzabile, non "nessun oggetto da confrontare per un bug
        // dell'oracolo" — la distinzione che questo intero test esiste per
        // fare. Qui il None e' quello giusto: sia canonical_remote sia
        // git_authority_for lo dicono per lo STESSO motivo (lo scheme non
        // e' quello che Git dispatcha), non per due motivi diversi.
        assert!(
            git_authority_for(mismatched_case).is_none(),
            "l'oracolo deve concordare che Git non ha un'opinione utilizzabile \
             per {mismatched_case:?} (nessun trasporto la riconoscerebbe)"
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

// ---- CONTROLLO NEGATIVO: la radice e' RUNTIME, non il path di compilazione --
//
// Il difetto misurato (2026-08-15): repo_root() calcolava la radice da
// `env!("CARGO_MANIFEST_DIR")` — macro di COMPILAZIONE. Il binario di test
// riusato dalla cache di un worktree POI rimosso panicava col path morto, e il
// gate era verde o rosso a seconda di DOVE era stato compilato, non di cosa
// conteneva l'albero. Le proprieta' da innestare, senza ricreare il worktree
// morto:
// 1. la radice si trova risalendo da QUALUNQUE punto del repo (cwd runtime);
// 2. ogni radice restituita ha un `.git` VERO — mai un'ipotesi;
// 3. quando la radice non c'e', il fallimento PARLA: fonti provate, causa
//    probabile, rimedio. Un panic senza rimedio in questa posizione si legge
//    come difetto di sanificazione — la caccia sbagliata.
#[test]
fn la_radice_e_runtime_e_il_fallimento_parla() {
    let pid = std::process::id();
    let tmp = std::env::temp_dir().join(format!("fork-gate-root-{pid}"));
    let _ = fs::remove_dir_all(&tmp);
    let _scratch = Scratch::new(vec![tmp.clone()]);
    // Mini repo con una sottodirectory: il gate parte tipicamente da
    // <root>/codex-rs/fork-gate, non dalla radice.
    fs::create_dir_all(tmp.join("codex-rs").join("fork-gate")).expect("creazione dir scratch");
    git(&tmp, &["init", "-q", "-b", "main", "."]);

    // (1) runtime: da una sottodirectory QUALSIASI si risale alla radice.
    let da_dentro = radice_da(&tmp.join("codex-rs").join("fork-gate"))
        .expect("la radice si trova risalendo da qualunque punto del repo");
    assert_eq!(da_dentro, tmp, "la radice trovata e' la directory del .git");

    // (2) mai un'ipotesi: se la ricerca restituisce qualcosa, ha un .git
    // VERO sopra. Da una dir senza .git (fino in cima, su una macchina
    // normale) e' None; l'asserzione vale comunque su macchine insolite —
    // la proprieta' e' la post-condizione, non il caso particolare.
    let fuori = std::env::temp_dir().join(format!("fork-gate-nogit-{pid}"));
    let _ = fs::remove_dir_all(&fuori);
    let _scratch2 = Scratch::new(vec![fuori.clone()]);
    fs::create_dir_all(&fuori).expect("creazione dir scratch");
    if let Some(r) = radice_da(&fuori) {
        assert!(
            r.join(".git").exists(),
            "mai una radice senza .git: {r:?}"
        );
    }

    // (3) il fallimento PARLA. Il messaggio e' puro: si esercita su input
    // sintetici senza dipendere dall'ambiente.
    let msg = msg_radice_non_trovata(&[fuori.clone()]);
    assert!(
        msg.contains(&fuori.display().to_string()),
        "il messaggio mostra la fonte provata: {msg}"
    );
    for (perche, pezzo) in [
        ("la causa probabile (binario di un worktree rimosso)", "worktree"),
        ("il rimedio (ricompilare)", "cargo clean -p fork-gate"),
        ("cosa NON e' (non sanificazione)", "Non e' un difetto di sanificazione"),
    ] {
        assert!(
            msg.contains(pezzo),
            "il messaggio deve dire {perche}: {msg}"
        );
    }
}

// ---- CONTROLLO NEGATIVO: FETCH_HEAD assente in un worktree, detto per intero --
//
// Il caso misurato (2026-08-15): FETCH_HEAD e' PER-WORKTREE, i ref
// refs/remotes/upstream/* sono CONDIVISI da tutti i checkout del repo. Un gate
// girato in un worktree dove nessuna fetch e' mai stata fatta vede i ref ma
// non puo' provarne la provenienza — e il rosso ambientale si legge come
// difetto di sanificazione se non lo si spiega. Il fixture ricostruisce il
// caso ESATTO con un worktree git vero (remote file://, nessuna rete):
// ref condivisi scaricati dal principale, FETCH_HEAD solo nel principale.
// Il rimedio indicato dal messaggio viene poi ESEGUITO nel worktree del
// fixture, per dimostrare che e' quello giusto: dopo la fetch fatta NEL
// worktree il SUO FETCH_HEAD esiste e la causa si sposta (nel fixture resta
// Err per l'URL file:// — non canonicalizzabile per scelta, un parser
// permissivo fabbrica prove false — ma non e' piu' «assente»: in produzione,
// col remote github reale, e' il punto in cui diventa verde).
#[test]
fn fetch_head_assente_in_un_worktree_e_un_fallimento_che_parla() {
    let pid = std::process::id();
    let base = std::env::temp_dir().join(format!("fork-gate-fh-{pid}"));
    let _ = fs::remove_dir_all(&base);
    let _scratch = Scratch::new(vec![base.clone()]);
    let up = base.join("up.git");
    let repo = base.join("repo");
    let wt = base.join("wt");
    fs::create_dir_all(&repo).expect("creazione dir scratch");

    // Repo con un commit; un bare clone fara' da upstream (file://, no rete).
    git(&repo, &["init", "-q", "-b", "main", "."]);
    git(&repo, &["config", "user.name", "Gate Test"]);
    git(&repo, &["config", "user.email", "gate@test.local"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    git(&repo, &["commit", "--allow-empty", "-q", "-m", "base"]);
    git(
        &base,
        &[
            "clone",
            "-q",
            "--bare",
            repo.display().to_string().as_str(),
            up.display().to_string().as_str(),
        ],
    );

    // La fetch avviene NEL REPO PRINCIPALE: FETCH_HEAD nasce li', i ref
    // upstream diventano condivisi.
    git(
        &repo,
        &[
            "remote",
            "add",
            "upstream",
            format!("file://{}", up.display()).as_str(),
        ],
    );
    git(&repo, &["fetch", "-q", "upstream"]);
    let _ = git_try(&repo, &["update-ref", "-d", "refs/remotes/upstream/HEAD"]);

    // Worktree dal principale: vede i ref condivisi, non ha FETCH_HEAD.
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            wt.display().to_string().as_str(),
            "HEAD",
        ],
    );
    let urefs: Vec<String> = git(
        &wt,
        &["for-each-ref", "--format=%(refname)", "refs/remotes/upstream"],
    )
    .lines()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
    assert!(
        !urefs.is_empty(),
        "i ref upstream sono condivisi: il worktree li vede senza fetch propria"
    );
    let fh_rel = git(&wt, &["rev-parse", "--git-path", "FETCH_HEAD"])
        .trim()
        .to_string();
    let fh_path = if Path::new(&fh_rel).is_absolute() {
        PathBuf::from(&fh_rel)
    } else {
        wt.join(&fh_rel)
    };
    assert!(
        !fh_path.exists(),
        "il FETCH_HEAD del worktree non esiste: nessuna fetch e' mai stata fatta qui"
    );

    // Il gate in questo worktree: Err PARLANTE, non un generico «non leggibile».
    let expected = format!("{}/{}", UPSTREAM_HOST, UPSTREAM_REPO);
    let prov = provenienza_refs_provata(&wt, &expected);
    let msg = prov
        .expect_err("FETCH_HEAD assente: la provenienza non e' provabile, Err non Ok")
        .join(" ");
    let fh_str = fh_path.display().to_string();
    for (perche, pezzo) in [
        (
            "QUALE file manca (il path del FETCH_HEAD di QUESTO checkout)",
            fh_str.as_str(),
        ),
        ("la causa (FETCH_HEAD per-worktree vs ref condivisi)", "per-worktree"),
        ("il rimedio (fetch NEL checkout del gate)", "git fetch upstream"),
        ("cosa NON e' (non sanificazione, non i ref)", "Non e' un difetto di sanificazione"),
    ] {
        assert!(
            msg.contains(pezzo),
            "il messaggio deve dire {perche}: {msg}"
        );
    }

    // Il rimedio e' quello giusto: fetch NEL WORKTREE (file://, no rete) e il
    // SUO FETCH_HEAD compare.
    git(&wt, &["fetch", "-q", "upstream"]);
    assert!(
        fh_path.exists(),
        "la fetch fatta NEL worktree crea il FETCH_HEAD del worktree"
    );
    // E la causa si sposta: non e' piu' «assente» (nel fixture resta Err per
    // l'URL file://, non canonicalizzabile per scelta: e' la dimostrazione
    // che il problema era quello, non un altro).
    let prov2 = provenienza_refs_provata(&wt, &expected);
    let msg2 = prov2
        .expect_err("col remote file:// del fixture l'URL resta non canonicalizzabile")
        .join(" ");
    assert!(
        !msg2.contains("FETCH_HEAD assente"),
        "dopo la fetch nel worktree il problema non e' piu' l'assenza: {msg2}"
    );
}
