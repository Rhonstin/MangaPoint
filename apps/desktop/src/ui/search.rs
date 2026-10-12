//! Searching every installed source at once.

use super::widgets::{cover_key, load_cover, Chip, Cover, EmptyState, Icon};
use crate::logic::{self, SearchSort};
use crate::state::now_secs;
use crate::state::spawn_forever;
use crate::state::{AppState, Page, SearchView};
use dioxus::prelude::*;
use futures_util::StreamExt;
use library::Verdict;
use search::fanout::record_health;
use search::{rank_series, search_stream, RankContext, SearchEvent, SearchState};
use sources::SourceManager;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[component]
pub fn SearchPage() -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(|| state.search.peek().query.clone());
    let view = state.search.read().clone();
    let failed_text = view.failed.join(", ");
    let order = logic::sorted_order(&view.ranked, *state.search_sort.read());

    rsx! {
        h1 { "Find a series" }
        p { class: "muted", "Searches all your sources at once and shows each series once, with the best source chosen for you." }
        form {
            class: "searchbar",
            onsubmit: move |e| {
                e.prevent_default();
                let q = text.read().trim().to_string();
                if !q.is_empty() {
                    run_search(state, q);
                }
            },
            div { class: "input-icon",
                Icon { name: "search", size: 18 }
                input {
                    r#type: "search",
                    placeholder: "Title, for example Berserk",
                    value: "{text}",
                    autofocus: true,
                    oninput: move |e| text.set(e.value()),
                }
            }
            // Submitting the form (this button or Enter) starts the search; no separate click handler.
            button { class: "btn primary", r#type: "submit", disabled: view.searching,
                if view.searching { Icon { name: "loader-circle", size: 17 } } else { Icon { name: "search", size: 17 } }
                "Search"
            }
        }

        SourceFilters { query: view.query.clone() }

        if view.no_sources {
            div { class: "banner warn",
                "No sources are installed yet. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), "Add sources" }
            }
        }
        if view.none_selected {
            div { class: "banner warn",
                "Every source is turned off for searching. Open the Sources list above and tick the ones you want."
            }
        }
        if view.searching {
            p { class: "muted", "Searching… {view.answered} of {view.total} sources answered" }
        } else if !view.query.is_empty() && view.ranked.is_empty() && !view.no_sources && !view.none_selected {
            EmptyState {
                title: "The cat looked everywhere",
                message: format!("It found nothing for “{}”. Try another spelling, or add more sources.", view.query),
            }
        } else if view.query.is_empty() && !view.no_sources && !view.none_selected {
            EmptyState {
                title: "Shh… the cat is napping",
                message: "Type a title above to wake it up, and it will fetch your next read.",
                shelf: true,
            }
        }
        if !view.failed.is_empty() && !view.searching {
            p { class: "muted small",
                "These sources did not answer: {failed_text}. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), "Check sources" }
            }
        }

        div { class: "grid",
            for i in order.iter().copied() {
                SeriesCard { key: "{view.ranked[i].title}-{i}", index: i }
            }
        }
    }
}

/// Which sources a search covers, and how the results are ordered.
#[component]
fn SourceFilters(query: String) -> Element {
    let state = use_context::<AppState>();
    let mut open = use_signal(|| false);
    let mut changed = use_signal(|| false);
    let mut find = use_signal(String::new);
    let installed = use_resource(move || async move {
        let _ = state.sources_version.read();
        let lang = state.settings.read().language.clone();
        let client = state.client()?;
        SourceManager::new(&*client).installed(&lang).await.ok()
    });
    let excluded = state.settings.read().search_excluded_sources.clone();
    let sort = *state.search_sort.read();
    let health = state.health.read().clone();

    // (id, name, language, adult-only), by name.
    let mut list: Vec<(String, String, String, bool)> = match &*installed.read() {
        Some(Some(items)) => items
            .iter()
            .map(|i| {
                (
                    i.source.id.clone(),
                    i.source.name.clone(),
                    i.source.lang.clone(),
                    i.source.is_nsfw,
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    list.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    let total = list.len();
    let on = list
        .iter()
        .filter(|(id, ..)| !excluded.contains(id))
        .count();
    let label = if total == 0 {
        "Sources".to_string()
    } else if on == total {
        format!("All sources ({total})")
    } else {
        format!("{on} of {total} sources")
    };
    let needle = find.read().trim().to_lowercase();
    let shown: Vec<&(String, String, String, bool)> = list
        .iter()
        .filter(|(_, name, lang, _)| {
            needle.is_empty()
                || name.to_lowercase().contains(&needle)
                || lang.to_lowercase().contains(&needle)
        })
        .collect();
    let all_ids: Vec<String> = list.iter().map(|(id, ..)| id.clone()).collect();
    let passing: Vec<String> = list
        .iter()
        .filter(|(id, ..)| {
            health
                .latest(id)
                .is_some_and(|r| r.verdict == Verdict::Pass)
        })
        .map(|(id, ..)| id.clone())
        .collect();
    let (ids_none, ids_ok) = (all_ids.clone(), all_ids.clone());

    let mut set_excluded = move |excluded: Vec<String>| {
        let mut settings = state.settings;
        settings.write().search_excluded_sources = excluded;
        state.save_settings();
        changed.set(true);
    };
    let close = {
        let query = query.clone();
        move || {
            open.set(false);
            // Search again with the new choice, if there is something to search for.
            if changed() && !query.trim().is_empty() {
                run_search(state, query.trim().to_string());
            }
            changed.set(false);
        }
    };
    let mut close_toggle = close.clone();
    let mut close_done = close.clone();
    let mut close_backdrop = close;

    rsx! {
        div { class: "filters",
            button {
                class: if open() { "btn small on" } else { "btn small" },
                disabled: total == 0,
                onclick: move |_| { if open() { close_toggle() } else { open.set(true) } },
                Icon { name: "layers", size: 15 }
                "{label}"
                Icon { name: "chevron-down", size: 14 }
            }
            div { class: "sort",
                span { class: "muted small", "Sort by" }
                select {
                    value: "{sort.key()}",
                    onchange: move |e| { let mut s = state.search_sort; s.set(SearchSort::from_key(&e.value())); },
                    for s in SearchSort::ALL {
                        option { key: "{s.key()}", value: "{s.key()}", selected: s == sort, "{s.label()}" }
                    }
                }
            }
            if open() {
                div { class: "backdrop", onclick: move |_| close_backdrop() }
                div { class: "popover",
                    div { class: "row", style: "margin-bottom: 8px",
                        strong { class: "grow", "Search these sources" }
                        button { class: "btn ghost small", onclick: move |_| set_excluded(Vec::new()), "All" }
                        button { class: "btn ghost small", onclick: move |_| set_excluded(ids_none.clone()), "None" }
                        button { class: "btn ghost small", title: "Only the sources whose last test passed",
                            onclick: move |_| {
                                set_excluded(ids_ok.iter().filter(|id| !passing.contains(id)).cloned().collect())
                            },
                            "Tested OK"
                        }
                    }
                    input {
                        r#type: "search", placeholder: "Find a source or a language",
                        value: "{find}", oninput: move |e| find.set(e.value()),
                    }
                    div { class: "source-list",
                        for (id, name, lang, adult) in shown.iter().map(|t| (t.0.clone(), t.1.clone(), t.2.clone(), t.3)) {
                            {
                                let included = !excluded.contains(&id);
                                let verdict = health.latest(&id).map(|r| r.verdict);
                                let toggle_id = id.clone();
                                let current = excluded.clone();
                                rsx! {
                                    label { class: "source-row", key: "{id}",
                                        input {
                                            r#type: "checkbox", checked: included,
                                            onchange: move |_| {
                                                let mut next = current.clone();
                                                if let Some(at) = next.iter().position(|x| *x == toggle_id) { next.remove(at); } else { next.push(toggle_id.clone()); }
                                                set_excluded(next);
                                            },
                                        }
                                        span { class: "grow", "{name}" }
                                        span { class: "muted small", "{lang}" }
                                        if adult { Chip { text: "18+", kind: Some("warn".to_string()) } }
                                        {match verdict {
                                            Some(Verdict::Pass) => rsx! { Chip { text: "works", kind: Some("ok".to_string()) } },
                                            Some(Verdict::NeedsBrowser) | Some(Verdict::NeedsFlareSolverr) => rsx! { Chip { text: "needs help", kind: Some("warn".to_string()) } },
                                            Some(Verdict::Failed) => rsx! { Chip { text: "not working", kind: Some("bad".to_string()) } },
                                            None => rsx! {},
                                        }}
                                    }
                                }
                            }
                        }
                        if shown.is_empty() { p { class: "muted small", style: "padding: 8px", "No source matches that." } }
                    }
                    div { class: "row", style: "margin-top: 10px",
                        span { class: "muted small grow", "{on} of {total} selected" }
                        button { class: "btn primary small", onclick: move |_| close_done(), "Done" }
                    }
                }
            }
        }
    }
}

#[component]
fn SeriesCard(index: usize) -> Element {
    let state = use_context::<AppState>();
    let view = state.search.read();
    let Some(series) = view.ranked.get(index).cloned() else {
        return rsx! {};
    };
    drop(view);
    let best = series.best().clone();
    let cover = cover_key(&best.candidate.source_id, best.candidate.manga_id);
    let others = series.members.len() - 1;
    let chapters = best.enrichment.as_ref().and_then(|e| e.chapter_count);
    let for_click = series.clone();
    rsx! {
        button {
            class: "series-card",
            onclick: move |_| {
                super::work::open_series_from_search(state, for_click.clone());
            },
            Cover { key_id: cover }
            div { class: "meta",
                div { class: "series-title", "{series.title}" }
                div { class: "muted small", "{best.candidate.source_name}" }
                div { class: "row wrap", style: "margin-top: 6px",
                    if others > 0 {
                        Chip { text: format!("+{others} more source{}", if others == 1 { "" } else { "s" }) }
                    }
                    if let Some(n) = chapters {
                        Chip { text: format!("{n} ch.") }
                    }
                }
            }
        }
    }
}

/// Run a search across all installed sources, filling the results in as
/// each source answers.
pub fn run_search(state: AppState, query: String) {
    spawn_forever(async move {
        let Some(client) = state.client() else { return };
        let lang = state.settings.read().language.clone();
        let preferred = state.settings.read().preferred_sources.clone();

        let mut view = state.search;
        let generation = state.search.peek().generation + 1;
        view.set(SearchView {
            query: query.clone(),
            searching: true,
            generation,
            ..Default::default()
        });

        let installed = SourceManager::new(&*client)
            .installed(&lang)
            .await
            .unwrap_or_default();
        let excluded = state.settings.peek().search_excluded_sources.clone();
        let all: Vec<_> = installed.into_iter().map(|i| i.source).collect();
        let none_installed = all.is_empty();
        let sources: Vec<_> = all
            .into_iter()
            .filter(|s| !excluded.contains(&s.id))
            .collect();
        if !none_installed && sources.is_empty() {
            view.set(SearchView {
                query,
                none_selected: true,
                generation,
                ..Default::default()
            });
            return;
        }
        if sources.is_empty() {
            view.set(SearchView {
                query,
                no_sources: true,
                generation,
                ..Default::default()
            });
            return;
        }

        info!("Searching for \"{query}\" on {} source(s)", sources.len());
        let started = Instant::now();
        let mut search = SearchState::new(query.clone(), sources.len());
        let mut enrichment = HashMap::new();
        let mut stream = Box::pin(search_stream(
            &*client,
            sources,
            query.clone(),
            Duration::from_secs(20),
            8,
        ));
        while let Some(event) = stream.next().await {
            match &event {
                SearchEvent::Done { source, mangas, ms } => {
                    info!("  {}: {} result(s) in {ms} ms", source.name, mangas.len());
                }
                SearchEvent::Failed {
                    source, message, ..
                } => {
                    warn!("  {}: no answer ({message})", source.name);
                }
            }
            search.apply(&event);
            {
                let mut health = state.health;
                record_health(&mut health.write(), &event);
            }
            if !publish(
                state,
                generation,
                &search,
                &query,
                &enrichment,
                &lang,
                &preferred,
                true,
            ) {
                info!("Search for \"{query}\" was replaced by a newer search");
                return;
            }
        }
        drop(stream);

        // Look at the chapter lists of the best-looking series to rank their sources properly.
        let top: Vec<_> = state
            .search
            .read()
            .ranked
            .iter()
            .take(8)
            .flat_map(|s| s.members.iter().take(3).map(|m| m.candidate.clone()))
            .collect();
        info!(
            "Got {} result(s) in {:.1}s; checking chapter lists to pick the best source for each…",
            search.candidates().len(),
            started.elapsed().as_secs_f32()
        );
        enrichment = search::enrich(&*client, &top, Duration::from_secs(25), 4).await;
        let current = publish(
            state,
            generation,
            &search,
            &query,
            &enrichment,
            &lang,
            &preferred,
            false,
        );
        state.save_health();
        if !current {
            return;
        }
        info!(
            "Search finished: {} series in {:.1}s",
            state.search.read().ranked.len(),
            started.elapsed().as_secs_f32()
        );
    });
}

/// Show the results so far. Returns false, doing nothing, if a newer search
/// has replaced this one.
#[allow(clippy::too_many_arguments)]
fn publish(
    state: AppState,
    generation: u64,
    search: &SearchState,
    query: &str,
    enrichment: &HashMap<(String, i64), search::Enrichment>,
    lang: &str,
    preferred: &[String],
    still_searching: bool,
) -> bool {
    if state.search.peek().generation != generation {
        return false;
    }
    let health = state.health.read().clone();
    let ctx = RankContext {
        health: &health,
        language: lang,
        preferred_sources: preferred,
        now_ms: (now_secs() * 1000) as i64,
        enrichment,
    };
    let ranked = rank_series(search.clusters(), query, &ctx);

    // Fetch the covers of the first screenful.
    for series in ranked.iter().take(30) {
        let best = &series.best().candidate;
        if let Some(path) = &best.thumbnail {
            load_cover(
                state,
                cover_key(&best.source_id, best.manga_id),
                path.clone(),
            );
        }
    }

    let mut view = state.search;
    view.set(SearchView {
        query: query.to_string(),
        ranked,
        searching: still_searching,
        answered: search.answered,
        total: search.total_sources,
        failed: search
            .failures
            .iter()
            .map(|(s, _)| s.name.clone())
            .collect(),
        no_sources: false,
        none_selected: false,
        generation,
    });
    true
}
