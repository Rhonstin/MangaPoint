//! Installed sources, their test results, and adding new ones.

use super::ask_enable_browser;
use super::widgets::{Chip, Icon};
use crate::backend;
use crate::logic::{ago, verdict_label};
use crate::state::spawn_forever;
use crate::state::{now_secs, AppState, Modal, ModalAction};
use catalog::Source;
use dioxus::prelude::*;
use library::Verdict;
use sources::defaults::EXTENSION_TRUST_WARNING;
use sources::{check_source, test_many, CheckOptions, InstalledSource, Requires, SourceManager};
use std::time::Duration;
use tracing::{info, warn};

#[component]
pub fn SourcesPage() -> Element {
    let state = use_context::<AppState>();
    let mut store_url = use_signal(String::new);
    let mut filter = use_signal(String::new);

    let installed = use_resource(move || async move {
        let _ = state.sources_version.read();
        let lang = state.settings.read().language.clone();
        let client = state.client()?;
        SourceManager::new(&*client).installed(&lang).await.ok()
    });
    let available = use_resource(move || async move {
        let _ = state.sources_version.read();
        let client = state.client()?;
        client.fetch_extensions().await.ok()
    });

    let now = now_secs();
    let health = state.health.read().clone();
    let testing = state.testing.read().clone();
    let browser_on = state.settings.read().browser_support;
    let lang = state.settings.read().language.clone();

    let rows: Vec<Element> = match &*installed.read() {
        Some(Some(list)) => list
            .iter()
            .map(|item| source_row(state, item, &health, &testing, browser_on, now))
            .collect(),
        _ => Vec::new(),
    };
    let installed_count = rows.len();
    let loading = installed.read().is_none();

    // Extensions the user could add, matching the filter.
    let query = filter.read().trim().to_lowercase();
    let candidates: Vec<catalog::Extension> = match &*available.read() {
        Some(Some(all)) if query.len() >= 2 => all
            .iter()
            .filter(|e| !e.is_installed)
            .filter(|e| e.lang == lang || e.lang == "all")
            .filter(|e| e.name.to_lowercase().contains(&query))
            .take(40)
            .cloned()
            .collect(),
        _ => Vec::new(),
    };

    rsx! {
        h1 { "Sources" }
        p { class: "muted", "The websites MangaPoint searches. “Retest” checks that a source really works: it looks up a series, its chapters and a page image." }

        div { class: "card",
            div { class: "row",
                h2 { class: "grow", Icon { name: "layers", size: 17 } "Installed ({installed_count})" }
                button {
                    class: "btn small",
                    disabled: installed_count == 0 || !testing.is_empty(),
                    onclick: move |_| retest_all(state),
                    Icon { name: "refresh-cw", size: 14 }
                    "Retest all"
                }
            }
            if loading { p { class: "muted", "Loading…" } }
            if !loading && installed_count == 0 {
                p { class: "muted", "No sources yet. Install some below." }
            }
            {rows.into_iter()}
        }

        div { class: "card",
            h2 { Icon { name: "circle-check", size: 17 } "Recommended sources" }
            p { class: "muted small", "Tested by MangaPoint. These are the ones we know work." }
            for d in sources::DEFAULTS.iter() {
                {
                    let is_installed = match &*installed.read() {
                        Some(Some(list)) => list.iter().any(|i| i.default.is_some_and(|x| x.id == d.id)),
                        _ => true,
                    };
                    let needs_browser = d.requires == Requires::Browser;
                    let id = d.id;
                    rsx! {
                        if !is_installed {
                            div { class: "list-row", key: "{id}",
                                div { class: "grow",
                                    strong { "{d.name}" }
                                    if needs_browser { Chip { text: "needs browser support", kind: Some("warn".to_string()) } }
                                    div { class: "muted small", "{d.description}" }
                                }
                                button {
                                    class: "btn primary small",
                                    onclick: move |_| {
                                        if needs_browser && !state.settings.read().browser_support {
                                            ask_enable_browser(state, Some(id.to_string()));
                                        } else {
                                            spawn_forever(async move {
                                                if backend::install_default(state, id).await {
                                                    state.notify("Installed. Press Retest to check it.");
                                                }
                                                state.bump_sources();
                                            });
                                        }
                                    },
                                    Icon { name: "download", size: 14 }
                                    "Install"
                                }
                            }
                        }
                    }
                }
            }
        }

        div { class: "card",
            h2 { Icon { name: "plus", size: 17 } "Add more sources" }
            div { class: "banner warn small", "{EXTENSION_TRUST_WARNING}" }
            p { class: "muted small", "Add an extension store (an https address), then search it below." }
            form {
                class: "row",
                onsubmit: move |e| {
                    e.prevent_default();
                    let url = store_url.read().clone();
                    spawn_forever(async move {
                        let Some(client) = state.client() else { return };
                        let mut sources_state = state.sources.read().clone();
                        let result = SourceManager::new(&*client).add_store(&mut sources_state, &url).await;
                        let mut s = state.sources;
                        s.set(sources_state);
                        state.save_sources();
                        match result {
                            Ok(list) => state.notify(format!("Store added: {} extensions available.", list.len())),
                            Err(e) => state.error(format!("Could not add that store: {e}")),
                        }
                        state.bump_sources();
                    });
                    store_url.set(String::new());
                },
                input {
                    r#type: "url", class: "grow",
                    placeholder: "https://example.org/repo/index.pb",
                    value: "{store_url}",
                    oninput: move |e| store_url.set(e.value()),
                }
                button { class: "btn", r#type: "submit", Icon { name: "plus", size: 15 } "Add store" }
            }
            div { class: "spacer" }
            input {
                r#type: "search", style: "width: 100%",
                placeholder: "Search available sources by name…",
                value: "{filter}",
                oninput: move |e| filter.set(e.value()),
            }
            for ext in candidates.iter() {
                {
                    let pkg = ext.pkg_name.clone();
                    let name = ext.name.clone();
                    rsx! {
                        div { class: "list-row", key: "{pkg}",
                            div { class: "grow",
                                strong { "{name}" }
                                span { class: "muted small", " {ext.lang}" }
                                if ext.is_nsfw { Chip { text: "18+", kind: Some("warn".to_string()) } }
                            }
                            button {
                                class: "btn small",
                                onclick: move |_| {
                                    let pkg = pkg.clone();
                                    spawn_forever(async move {
                                        let Some(client) = state.client() else { return };
                                        let mut sources_state = state.sources.read().clone();
                                        let result = SourceManager::new(&*client).install_custom(&mut sources_state, &pkg).await;
                                        let mut s = state.sources;
                                        s.set(sources_state);
                                        state.save_sources();
                                        match result {
                                            Ok(()) => state.notify("Installed. Press Retest to check it."),
                                            Err(e) => state.error(format!("Could not install it: {e}")),
                                        }
                                        state.bump_sources();
                                    });
                                },
                                Icon { name: "download", size: 14 }
                                "Install"
                            }
                        }
                    }
                }
            }
            if query.len() >= 2 && candidates.is_empty() {
                p { class: "muted small", "Nothing matches “{query}” in your stores." }
            }
        }
    }
}

fn source_row(
    state: AppState,
    item: &InstalledSource,
    health: &library::HealthDb,
    testing: &std::collections::HashSet<String>,
    browser_on: bool,
    now: u64,
) -> Element {
    let source = item.source.clone();
    let id = source.id.clone();
    let last = health.latest(&id).cloned();
    let is_testing = testing.contains(&id);
    let reference = item
        .default
        .and_then(|d| d.reference_title)
        .map(String::from);
    let pkg = item.extension.pkg_name.clone();
    let name = source.name.clone();
    let needs_browser = last
        .as_ref()
        .is_some_and(|r| r.verdict == Verdict::NeedsBrowser)
        && !browser_on;
    let (chip_text, chip_kind) = match &last {
        None => ("not tested yet".to_string(), None),
        Some(r) => (
            verdict_label(r.verdict).to_string(),
            Some(
                match r.verdict {
                    Verdict::Pass => "ok",
                    Verdict::NeedsBrowser => "warn",
                    _ => "bad",
                }
                .to_string(),
            ),
        ),
    };

    rsx! {
        div { class: "list-row", key: "{id}", style: "align-items: flex-start",
            div { class: "grow",
                div { class: "row wrap",
                    strong { "{name}" }
                    span { class: "muted small", "{source.lang}" }
                    Chip { text: chip_text, kind: chip_kind }
                    if let Some(d) = item.default {
                        if d.verified_on.starts_with("20") {
                            span { class: "muted small", "verified {d.verified_on}" }
                        }
                    }
                }
                if is_testing {
                    div { class: "muted small", "Testing…" }
                } else if let Some(r) = &last {
                    div { class: "muted small", "{r.message} Tested {ago(now, r.at)}, {r.total_ms()} ms." }
                }
            }
            if needs_browser {
                button { class: "btn small", onclick: move |_| ask_enable_browser(state, None), "Turn on browser support" }
            }
            button {
                class: "btn small",
                disabled: is_testing,
                onclick: {
                    let source = source.clone();
                    let reference = reference.clone();
                    move |_| retest(state, source.clone(), reference.clone())
                },
                Icon { name: "refresh-cw", size: 14 }
                "Retest"
            }
            button {
                class: "btn small danger",
                onclick: move |_| {
                    state.ask(Modal {
                        title: "Remove this source?".into(),
                        body: format!("{name} will no longer be searched. You can install it again later."),
                        confirm: "Remove".into(),
                        cancel: "Keep".into(),
                        danger: true,
                        action: ModalAction::RemoveExtension(pkg.clone()),
                    });
                },
                Icon { name: "trash", size: 14 }
                "Remove"
            }
        }
    }
}

/// Test one source and record the result.
pub fn retest(state: AppState, source: Source, reference_title: Option<String>) {
    spawn_forever(async move {
        let Some(client) = state.client() else { return };
        let mut testing = state.testing;
        testing.write().insert(source.id.clone());
        info!("Testing source {}…", source.name);
        let options = CheckOptions {
            reference_title,
            ..Default::default()
        };
        let record = check_source(&*client, &source, &options).await;
        log_test(&source.name, &record);
        {
            let mut health = state.health;
            health.write().record_test(&source.id, &source.name, record);
        }
        state.save_health();
        testing.write().remove(&source.id);
        state.bump_sources();
    });
}

/// One line per test: the verdict, how long it took, and why if it failed.
fn log_test(name: &str, record: &library::TestRecord) {
    let label = verdict_label(record.verdict);
    if record.verdict == Verdict::Pass {
        info!("  {name}: {label} ({} ms)", record.total_ms());
    } else {
        warn!("  {name}: {label}. {}", record.message);
    }
}

fn retest_all(state: AppState) {
    spawn_forever(async move {
        let Some(client) = state.client() else { return };
        let lang = state.settings.read().language.clone();
        let Ok(installed) = SourceManager::new(&*client).installed(&lang).await else {
            return;
        };
        let jobs: Vec<(Source, CheckOptions)> = installed
            .iter()
            .map(|i| {
                (
                    i.source.clone(),
                    CheckOptions {
                        reference_title: i
                            .default
                            .and_then(|d| d.reference_title)
                            .map(String::from),
                        step_timeout: Duration::from_secs(45),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let mut testing = state.testing;
        info!("Testing all {} installed source(s)…", jobs.len());
        for (s, _) in &jobs {
            testing.write().insert(s.id.clone());
        }
        let _ = test_many(&*client, jobs, 3, |source, record| {
            log_test(&source.name, record);
            let mut health = state.health;
            health
                .write()
                .record_test(&source.id, &source.name, record.clone());
            testing.write().remove(&source.id);
            state.bump_sources();
        })
        .await;
        state.save_health();
    });
}
