//! Crate ausiliario del gate di sanificazione pre-publish per codex-vl.
//!
//! Non contiene codice di runtime: esiste solo per ospitare i test di
//! integrazione in `tests/`, che verificano che l'albero tracciato e la
//! storia del fork non portino tracce da sanificare prima di una
//! pubblicazione. Specchia il rationale del test `published-tree-is-clean`
//! di nexuscrew, adattandolo a un fork Rust con storia upstream.
