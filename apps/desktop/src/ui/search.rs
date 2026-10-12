//! Searching every installed source at once.

use super::widgets::{cover_key, load_cover, Chip, Cover, Icon};
use crate::state::now_secs;
use crate::state::spawn_forever;
use crate::state::{AppState, Page, SearchView};
use dioxus::prelude::*;
use futures_util::StreamExt;
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

        if view.no_sources {
            div { class: "banner warn",
                "No sources are installed yet. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), "Add sources" }
            }
        }
        if view.searching {
            p { class: "muted", "Searching… {view.answered} of {view.total} sources answered" }
        } else if !view.query.is_empty() && view.ranked.is_empty() && !view.no_sources {
            p { class: "muted", "No results for “{view.query}”. Try another spelling, or add more sources." }
        }
        if !view.failed.is_empty() && !view.searching {
            p { class: "muted small",
                "These sources did not answer: {failed_text}. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), "Check sources" }
            }
        }

        div { class: "grid",
            for (i, series) in view.ranked.iter().enumerate() {
                SeriesCard { key: "{series.title}-{i}", index: i }
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
                let mut s = state.series;
                s.set(Some(for_click.clone()));
                state.go(Page::Series);
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
        let sources: Vec<_> = installed.into_iter().map(|i| i.source).collect();
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
        generation,
    });
    true
}
