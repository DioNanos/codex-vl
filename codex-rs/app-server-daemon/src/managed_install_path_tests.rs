use pretty_assertions::assert_eq;

#[test]
fn fork_root_is_isolated_from_upstream_installs() {
    let home = tempfile::TempDir::new().expect("home");
    let fork_root = super::package_root(home.path());
    // Attesa LETTERALE del root del fork: la funzione in prova non puo'
    // certificare se stessa.
    let fork_root_expected = home.path().join("packages/app-server-daemon-vl");
    assert_eq!(fork_root, fork_root_expected);
    // I due alberi upstream (standalone e app-server-daemon): release, legacy,
    // marker e file di state/pid/log completi. Nessuno di questi elementi
    // deve spostare il root del fork o la sua selezione.
    let upstream_standalone = home.path().join("packages/standalone");
    let upstream_daemon = home.path().join("packages/app-server-daemon");
    for upstream in [upstream_standalone.clone(), upstream_daemon.clone()] {
        let current = upstream.join("current");
        let legacy = current.join(super::managed_codex_file_name());
        std::fs::create_dir_all(current.join("bin")).expect("upstream current");
        std::fs::write(current.join("bin/codex"), b"upstream packaged").expect("upstream packaged");
        std::fs::write(&legacy, b"upstream legacy").expect("upstream legacy");
        std::fs::write(
            upstream.join("auto-update-version"),
            b"0.160.0-x86_64-unknown-linux-musl",
        )
        .expect("upstream marker");
    }
    let upstream_state = home.path().join("app-server-daemon");
    std::fs::create_dir_all(&upstream_state).expect("upstream state");
    // Attesa letterale della selezione nel root fork: la funzione in prova non
    // puo' certificare la propria selezione.
    let fork_selection_expected = fork_root_expected
        .join("current/bin")
        .join(super::managed_codex_file_name());
    for name in [
        "settings.json",
        "daemon.lock",
        "app-server.pid.lock",
        "app-server.stderr.log",
        "app-server.pid",
        "daemon.pid",
        "daemon.stderr.log",
    ] {
        std::fs::write(upstream_state.join(name), b"").expect("upstream state file");
        // Ogni singola mutazione upstream lascia il root del fork e la
        // selezione del fork al loro posto.
        assert_eq!(
            super::package_root(home.path()),
            fork_root_expected,
            "root fork spostato dalla mutazione upstream: {name}"
        );
        assert_eq!(
            super::managed_codex_bin(home.path()),
            fork_selection_expected,
            "selezione spostata dalla mutazione upstream: {name}"
        );
    }

    // Il root resta il namespace -vl e la selezione resta nel current del fork.
    assert_eq!(super::package_root(home.path()), fork_root_expected);
    let fork_packaged = fork_root
        .join("current/bin")
        .join(super::managed_codex_file_name());
    assert_eq!(super::managed_codex_bin(home.path()), fork_packaged);

    // Fallback legacy SOLO dentro il root fork: current/codex esistente ->
    // selezione legacy del fork; aggiunto current/bin/codex -> packaged.
    let fork_legacy = fork_root
        .join("current")
        .join(super::managed_codex_file_name());
    std::fs::create_dir_all(fork_legacy.parent().unwrap()).expect("fork current");
    std::fs::write(&fork_legacy, b"fork legacy").expect("fork legacy");
    assert_eq!(super::managed_codex_bin(home.path()), fork_legacy);
    std::fs::create_dir_all(fork_packaged.parent().unwrap()).expect("fork bin");
    std::fs::write(&fork_packaged, b"fork packaged").expect("fork packaged");
    assert_eq!(super::managed_codex_bin(home.path()), fork_packaged);

    // Root fork non-directory: non adotta alcuna installazione upstream.
    #[cfg(unix)]
    {
        std::fs::remove_dir_all(&fork_root).expect("remove fork root");
        std::fs::write(&fork_root, b"not a directory").expect("fork root as file");
        assert_eq!(super::package_root(home.path()), fork_root_expected);
        assert_eq!(
            super::managed_codex_bin(home.path()),
            fork_root
                .join("current/bin")
                .join(super::managed_codex_file_name())
        );
        std::fs::remove_file(&fork_root).expect("cleanup fork root");
    }

    // Gli alberi upstream restano intatti: OGNI sentinel predisposto e`
    // confrontato per bytes, non solo per esistenza.
    assert_eq!(
        std::fs::read(upstream_standalone.join("current/bin/codex")).unwrap(),
        b"upstream packaged"
    );
    assert_eq!(
        std::fs::read(upstream_standalone.join("current/codex")).unwrap(),
        b"upstream legacy"
    );
    assert_eq!(
        std::fs::read(upstream_daemon.join("current/bin/codex")).unwrap(),
        b"upstream packaged"
    );
    assert_eq!(
        std::fs::read(upstream_daemon.join("current/codex")).unwrap(),
        b"upstream legacy"
    );
    assert_eq!(
        std::fs::read(upstream_standalone.join("auto-update-version")).unwrap(),
        b"0.160.0-x86_64-unknown-linux-musl"
    );
    assert_eq!(
        std::fs::read(upstream_daemon.join("auto-update-version")).unwrap(),
        b"0.160.0-x86_64-unknown-linux-musl"
    );
    for name in [
        "settings.json",
        "daemon.lock",
        "app-server.pid.lock",
        "app-server.stderr.log",
        "app-server.pid",
        "daemon.pid",
        "daemon.stderr.log",
    ] {
        assert_eq!(
            std::fs::read(upstream_state.join(name)).unwrap(),
            b"",
            "sentinel upstream alterato: {name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn updater_only_runs_for_stable_installer_owned_releases() {
    // Root upstream (packages/standalone): NON e` di proprieta` del fork — il
    // predicato resta falso anche con release stable e marker corrispondente.
    let home = tempfile::TempDir::new().expect("home");
    let upstream_state = home.path().join("app-server-daemon");
    std::fs::create_dir(&upstream_state).unwrap();
    std::fs::write(upstream_state.join("app-server.stderr.log"), b"").unwrap();
    let upstream_standalone = home.path().join("packages/standalone");
    let upstream_current = upstream_standalone.join("current");
    let upstream_release = upstream_standalone.join("releases/0.150.0-aarch64-apple-darwin");
    let upstream_managed = upstream_release.join("bin/codex");
    std::fs::create_dir_all(upstream_managed.parent().expect("bin parent")).expect("release");
    std::fs::write(&upstream_managed, b"stable upstream").expect("managed bin");
    std::os::unix::fs::symlink(&upstream_release, &upstream_current).expect("current release");
    assert!(!super::is_stable_standalone_release(
        home.path(),
        &upstream_managed
    ));
    let upstream_marker = upstream_standalone.join("auto-update-version");
    std::fs::write(
        &upstream_marker,
        upstream_release
            .file_name()
            .expect("release name")
            .as_encoded_bytes(),
    )
    .expect("latest selection");
    assert!(
        !super::is_stable_standalone_release(home.path(), &upstream_managed),
        "l'ownership del fork e` il root -vl, non packages/standalone"
    );

    // Root del fork: stable numerica + marker corrispondente -> true.
    let fork_root = super::package_root(home.path());
    let release = fork_root.join("releases/0.160.0-x86_64-unknown-linux-musl");
    let current = fork_root.join("current");
    let managed = release.join("bin/codex");
    std::fs::create_dir_all(managed.parent().expect("bin parent")).expect("release");
    std::fs::write(&managed, b"stable fork").expect("managed bin");
    std::fs::write(
        release.join("codex-package.json"),
        r#"{"layoutVersion":1,"variant":"codex-vl","target":"x86_64-unknown-linux-musl","entrypoint":"bin/codex"}"#,
    )
    .expect("fork manifest");
    std::os::unix::fs::symlink(&release, &current).expect("current release");
    assert!(!super::is_stable_standalone_release(home.path(), &managed));
    let marker = fork_root.join("auto-update-version");
    std::fs::write(
        &marker,
        release
            .file_name()
            .expect("release name")
            .as_encoded_bytes(),
    )
    .expect("latest selection");
    assert!(super::is_stable_standalone_release(home.path(), &managed));
    std::fs::write(&marker, b"0.159.0-x86_64-unknown-linux-musl").expect("stale selection");
    assert!(!super::is_stable_standalone_release(home.path(), &managed));
    std::fs::remove_file(&marker).expect("pinned selection");
    assert!(!super::is_stable_standalone_release(home.path(), &managed));

    let alpha = fork_root.join("releases/0.161.0-alpha.1-x86_64-unknown-linux-musl");
    let alpha_managed = alpha.join("bin/codex");
    std::fs::create_dir_all(alpha_managed.parent().expect("alpha bin parent"))
        .expect("alpha release");
    std::fs::write(&alpha_managed, b"alpha").expect("alpha bin");
    std::fs::remove_file(&current).expect("remove current");
    std::os::unix::fs::symlink(&alpha, &current).expect("current alpha");
    // Rifiuto della VERSIONE (alpha): il marker corrisponde alla release, ma
    // il predicato esige una stable numerica.
    std::fs::write(
        &marker,
        alpha
            .file_name()
            .expect("alpha release name")
            .as_encoded_bytes(),
    )
    .expect("alpha marker");
    assert!(!super::is_stable_standalone_release(
        home.path(),
        &alpha_managed
    ));
    // Caso distinto: MARKER ASSENTE, a release invariata.
    std::fs::remove_file(&marker).expect("remove marker for missing-marker case");
    assert!(!super::is_stable_standalone_release(
        home.path(),
        &alpha_managed
    ));

    let local = fork_root.join("local-main");
    let local_managed = local.join("bin/codex");
    std::fs::create_dir_all(local_managed.parent().expect("local bin parent"))
        .expect("local build");
    std::fs::write(&local_managed, b"local").expect("local bin");
    std::fs::remove_file(&current).expect("remove current");
    std::os::unix::fs::symlink(&local, &current).expect("current local build");
    assert!(!super::is_stable_standalone_release(
        home.path(),
        &local_managed
    ));

    // Versione reale del fork (suffisso -vl.1): il predicato richiede tre
    // componenti numeriche -> resta fuori dall'updater standalone.
    let real = fork_root.join("releases/0.160.0-vl.1-x86_64-unknown-linux-musl");
    let real_managed = real.join("bin/codex");
    std::fs::create_dir_all(real_managed.parent().expect("bin parent")).expect("real release");
    std::fs::write(&real_managed, b"real fork release").expect("real bin");
    std::fs::remove_file(&current).expect("remove current");
    std::os::unix::fs::symlink(&real, &current).expect("current real");
    std::fs::write(
        &marker,
        real.file_name().expect("release name").as_encoded_bytes(),
    )
    .expect("real marker");
    assert!(!super::is_stable_standalone_release(
        home.path(),
        &real_managed
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn older_managed_binary_does_not_claim_updater_support() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::TempDir::new().expect("home");
    let binary = temp.path().join("codex");
    std::fs::write(&binary, b"#!/bin/sh\nexit 2\n").expect("older binary");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("executable binary");
    assert!(!super::supports_daemon_update_loop(&binary).await);
    std::fs::write(&binary, b"#!/bin/sh\nexit 0\n").expect("newer binary");
    assert!(super::supports_daemon_update_loop(&binary).await);
}
