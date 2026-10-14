//! Searching every installed source at once.

use super::widgets::{cover_key, load_cover, Busy, Chip, Cover, EmptyState, Icon, Loading};
use crate::logic::{self, ExplicitFilter, SearchFilters, SearchSort, COMMON_GENRES, SERIES_TYPES};
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

/// A source as the search page shows it.
#[derive(Clone, PartialEq)]
struct SourceInfo {
    id: String,
    name: String,
    lang: String,
    adult: bool,
}

#[component]
pub fn SearchPage() -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(|| state.search.peek().query.clone());
    let mut open = use_signal(|| false);
    let mut filters_open = use_signal(|| false);
    let changed = use_signal(|| false);
    let view = state.search.read().clone();
    let failed_text = view.failed.join(", ");
    let sort = *state.search_sort.read();
    let order = logic::sorted_order(&view.ranked, sort);

    let installed = use_resource(move || async move {
        let _ = state.sources_version.read();
        let lang = state.settings.read().language.clone();
        let client = state.client()?;
        SourceManager::new(&*client).installed(&lang).await.ok()
    });
    let excluded = state.settings.read().search_excluded_sources.clone();
    let mut sources: Vec<SourceInfo> = match &*installed.read() {
        Some(Some(items)) => items
            .iter()
            .map(|i| SourceInfo {
                id: i.source.id.clone(),
                name: i.source.name.clone(),
                lang: i.source.lang.clone(),
                adult: i.source.is_nsfw,
            })
            .collect(),
        _ => Vec::new(),
    };
    sources.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let total = sources.len();
    // Narrow the results by what is known of each.
    let filters = state.search_filters.read().clone();
    let all_count = order.len();
    let (order, pending) = {
        let adult: std::collections::HashSet<&str> = sources
            .iter()
            .filter(|s| s.adult)
            .map(|s| s.id.as_str())
            .collect();
        let tags = state.search_tags.read();
        let key = |i: usize| {
            let best = &view.ranked[i].best().candidate;
            (best.source_id.clone(), best.manga_id)
        };
        let kept: Vec<usize> = order
            .into_iter()
            .filter(|&i| {
                let k = key(i);
                filters.allows(tags.get(&k).map(Vec::as_slice), adult.contains(k.0.as_str()))
            })
            .collect();
        let pending = if filters.needs_tags() {
            (0..view.ranked.len().min(60))
                .filter(|&i| !tags.contains_key(&key(i)))
                .count()
        } else {
            0
        };
        (kept, pending)
    };
    // Types and genres are read from the details of the results, which are
    // looked up once a filter needs them.
    use_effect(move || {
        if !state.search_filters.read().needs_tags() {
            return;
        }
        let wanted: Vec<(String, i64)> = state
            .search
            .read()
            .ranked
            .iter()
            .take(60)
            .map(|s| (s.best().candidate.source_id.clone(), s.best().candidate.manga_id))
            .filter(|k| !state.search_tags.peek().contains_key(k))
            .collect();
        if !wanted.is_empty() {
            fetch_tags(state, wanted);
        }
    });
    let filter_labels = filters.labels();
    let filter_button = if filter_labels.is_empty() {
        "Filters".to_string()
    } else {
        format!("Filters · {}", filter_labels.len())
    };
    let included: Vec<SourceInfo> = sources
        .iter()
        .filter(|s| !excluded.contains(&s.id))
        .cloned()
        .collect();
    let filtered = included.len() != total;
    let sources_label = if total == 0 {
        "Sources".to_string()
    } else if filtered {
        format!("{} of {total} Sources", included.len())
    } else {
        format!("{total} Sources")
    };

    // Searching again with the sources as they are now, if there is a search.
    let search_again = move || {
        let query = state.search.peek().query.clone();
        if !query.trim().is_empty() {
            run_search(state, query.trim().to_string());
        }
    };
    // Change which sources are searched.
    let set_excluded = move |list: Vec<String>| {
        let mut settings = state.settings;
        settings.write().search_excluded_sources = list;
        state.save_settings();
    };
    // Closing the panel searches again if the choice changed.
    let close_panel = move || {
        let mut open = open;
        let mut changed = changed;
        open.set(false);
        if changed() {
            search_again();
        }
        changed.set(false);
    };

    // Before anything is searched the box sits in the middle of the window, under
    // the cat. Searching moves it up and widens it (see style.css).
    let idle = view.query.is_empty() && !view.searching;
    rsx! {
      div { class: if idle { "search-stage idle" } else { "search-stage" },
        div { class: "search-hero",
            super::ocean::OceanCat {}
            h2 { "What are we reading?" }
            p { class: "muted", "Type a title below and the cat will go looking for it." }
        }
        // The search box is all there is at the top. It is a grid of two things
        // in one place: an invisible copy of the full-size box, which holds the
        // space (so nothing below moves and the pointer is never left outside
        // while the box shrinks), and the visible box, which is as small or as
        // big as it needs to be.
        div { class: if open() || filters_open() { "searchwrap open" } else { "searchwrap" },
            div { class: "searchbox searchbox-ghost", "aria-hidden": "true",
                div { class: "searchbox-input", "\u{a0}" }
                div { class: "searchbox-row",
                    div { class: "pills-flow",
                        span { class: "ghost-sort", "Sort: {sort.label()}" }
                        for label in filter_labels.iter() {
                            span { class: "ghost-pill", key: "{label}", "{label}" }
                        }
                        for source in included.iter() {
                            span { class: "ghost-pill", key: "{source.id}", "{source.name}" }
                        }
                    }
                    span { class: "search-circle" }
                }
            }
            div { class: "cardwrap",
                form {
                    class: "searchbox",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let q = text.read().trim().to_string();
                        if !q.is_empty() {
                            run_search(state, q);
                        }
                    },
                    input {
                        class: "searchbox-input",
                        r#type: "search",
                        placeholder: "Type a manga, manhua, or manhwa here…",
                        value: "{text}",
                        autofocus: true,
                        oninput: move |e| text.set(e.value()),
                    }
                    div { class: "searchbox-row",
                        // The sort and the sources flow and wrap together, so a
                        // second row starts under the sort pill. Pointing anywhere
                        // at the box, typing in it, or opening the panel folds the
                        // source pills into one dropdown (see style.css).
                        div { class: "pills-flow",
                            div { class: "sort-pill",
                                select {
                                    value: "{sort.key()}",
                                    title: "How the results are ordered",
                                    onchange: move |e| { let mut s = state.search_sort; s.set(SearchSort::from_key(&e.value())); },
                                    for option_sort in SearchSort::ALL {
                                        option { key: "{option_sort.key()}", value: "{option_sort.key()}", selected: option_sort == sort, "Sort: {option_sort.label()}" }
                                    }
                                }
                                span { class: "chev", Icon { name: "chevron-down", size: 14 } }
                            }
                            button {
                                class: "pill-btn fil-btn", r#type: "button",
                                title: "Narrow the results by type, explicit content and genre",
                                onclick: move |_| {
                                    if open() { close_panel() }
                                    filters_open.set(!filters_open());
                                },
                                "{filter_button}"
                                Icon { name: "chevron-down", size: 14 }
                            }
                            for label in filter_labels.iter() {
                                span { class: "src-pill fil-pill", key: "{label}", "{label}" }
                            }
                            button {
                                class: "pill-btn", r#type: "button",
                                title: "Choose which sources to search",
                                disabled: total == 0,
                                onclick: move |_| { if open() { close_panel() } else { filters_open.set(false); open.set(true) } },
                                "{sources_label}"
                                Icon { name: "chevron-down", size: 14 }
                            }
                            for source in included.iter() {
                                span { class: "src-pill", key: "{source.id}", "{source.name}" }
                            }
                            if total > 0 && included.is_empty() {
                                span { class: "muted small", style: "padding: 5px 4px", "No source is chosen. Point at the box and open the sources list." }
                            }
                        }
                        // Submitting the form (this button or Enter) starts the search.
                        button { class: "search-circle", r#type: "submit", title: "Search", disabled: view.searching,
                            Icon { name: "search", size: 18 }
                        }
                    }
                }
                if filters_open() {
                    div { class: "backdrop", onclick: move |_| filters_open.set(false) }
                    FilterPanel { pending, on_close: move |()| filters_open.set(false) }
                }
                if open() {
                    div { class: "backdrop", onclick: move |_| close_panel() }
                    SourcePanel {
                        sources: sources.clone(),
                        excluded: excluded.clone(),
                        on_change: move |list: Vec<String>| { set_excluded(list); let mut changed = changed; changed.set(true); },
                        on_close: move |()| close_panel(),
                    }
                }
            }
        }

        if view.no_sources {
            div { class: "banner warn",
                "No sources are installed yet. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), Icon { name: "plus", size: 15 } "Add sources" }
            }
        }
        if view.none_selected {
            div { class: "banner warn",
                "Every source is turned off for searching. Use the filter button and tick the ones you want."
            }
        }
        if view.searching {
            Loading { label: format!("Searching… {} of {} sources answered", view.answered, view.total) }
        } else if !view.query.is_empty() && view.ranked.is_empty() && !view.no_sources && !view.none_selected {
            EmptyState {
                title: "The cat looked everywhere",
                message: format!("It found nothing for “{}”. Try another spelling, or add more sources.", view.query),
                scene: "search",
            }
        }
        if !filters.is_empty() && !view.ranked.is_empty() && !view.searching {
            div { class: "row wrap filter-note",
                span { class: "muted small", "Showing {order.len()} of {all_count} results" }
                if pending > 0 { Busy { label: "Checking the genres of {pending} more…", size: 36 } }
                button { class: "btn small ghost", onclick: move |_| { let mut f = state.search_filters; f.set(SearchFilters::default()); },
                    "Clear filters"
                }
            }
        }
        if !view.failed.is_empty() && !view.searching {
            p { class: "muted small",
                "These sources did not answer: {failed_text}. "
                button { class: "btn small", onclick: move |_| state.go(Page::Sources), Icon { name: "refresh-cw", size: 15 } "Check sources" }
            }
        }

        div { class: "grid",
            for i in order.iter().copied() {
                SeriesCard { key: "{view.ranked[i].title}-{i}", index: i }
            }
        }
      }
    }
}

/// The panel for choosing the sources to search, opened by the filter button.
#[component]
fn SourcePanel(
    sources: Vec<SourceInfo>,
    excluded: Vec<String>,
    on_change: EventHandler<Vec<String>>,
    on_close: EventHandler<()>,
) -> Element {
    let state = use_context::<AppState>();
    let mut find = use_signal(String::new);
    let health = state.health.read().clone();
    let total = sources.len();
    let on = sources.iter().filter(|s| !excluded.contains(&s.id)).count();
    let needle = find.read().trim().to_lowercase();
    let shown: Vec<&SourceInfo> = sources
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.name.to_lowercase().contains(&needle)
                || s.lang.to_lowercase().contains(&needle)
        })
        .collect();
    let all_ids: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
    let failing: Vec<String> = sources
        .iter()
        .filter(|s| {
            !health
                .latest(&s.id)
                .is_some_and(|r| r.verdict == Verdict::Pass)
        })
        .map(|s| s.id.clone())
        .collect();

    rsx! {
        div { class: "popover",
            div { class: "popover-head",
                strong { class: "grow", "Search these sources" }
                span { class: "muted small", "{on} of {total}" }
            }
            div { class: "popover-quick",
                button { class: "pill small", r#type: "button", onclick: move |_| on_change.call(Vec::new()), "All" }
                button { class: "pill small", r#type: "button", onclick: move |_| on_change.call(all_ids.clone()), "None" }
                button { class: "pill small", r#type: "button", title: "Only the sources whose last test passed",
                    onclick: move |_| on_change.call(failing.clone()),
                    "Tested OK"
                }
            }
            input {
                r#type: "search", placeholder: "Find a source or a language",
                value: "{find}", oninput: move |e| find.set(e.value()),
            }
            div { class: "source-list",
                for source in shown.iter() {
                    {
                        let included = !excluded.contains(&source.id);
                        let verdict = health.latest(&source.id).map(|r| r.verdict);
                        let toggle_id = source.id.clone();
                        let current = excluded.clone();
                        rsx! {
                            label { class: "source-row", key: "{source.id}",
                                input {
                                    r#type: "checkbox", checked: included,
                                    onchange: move |_| {
                                        let mut next = current.clone();
                                        if let Some(at) = next.iter().position(|x| *x == toggle_id) { next.remove(at); } else { next.push(toggle_id.clone()); }
                                        on_change.call(next);
                                    },
                                }
                                span { class: "grow", "{source.name}" }
                                span { class: "muted small", "{source.lang}" }
                                if source.adult { Chip { text: "18+", kind: Some("warn".to_string()) } }
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
                button { class: "btn primary small", r#type: "button", onclick: move |_| on_close.call(()), "Done" }
            }
        }
    }
}

/// Look up the genres of some search results (a few at a time), so the type
/// and genre filters can tell them apart. A series whose details cannot be
/// read is kept with none, so it is not asked for again.
fn fetch_tags(state: AppState, wanted: Vec<(String, i64)>) {
    static ASKED: std::sync::Mutex<Vec<(String, i64)>> = std::sync::Mutex::new(Vec::new());
    let Some(client) = state.client() else { return };
    let todo: Vec<(String, i64)> = {
        let Ok(mut asked) = ASKED.lock() else { return };
        wanted
            .into_iter()
            .filter(|k| {
                if asked.contains(k) {
                    false
                } else {
                    asked.push(k.clone());
                    true
                }
            })
            .collect()
    };
    if todo.is_empty() {
        return;
    }
    spawn_forever(async move {
        futures_util::stream::iter(todo)
            .for_each_concurrent(4, |key| {
                let client = client.clone();
                async move {
                    let tags = match tokio::time::timeout(
                        Duration::from_secs(20),
                        client.manga_details(key.1),
                    )
                    .await
                    {
                        Ok(Ok(details)) => details.genre,
                        _ => Vec::new(),
                    };
                    let mut all = state.search_tags;
                    all.write().insert(key, tags);
                }
            })
            .await;
    });
}

fn set_filters(state: AppState, filters: SearchFilters) {
    let mut f = state.search_filters;
    f.set(filters);
}

/// Add a word to a list, or take it out if it is there.
fn toggled(list: &[String], word: &str) -> Vec<String> {
    let mut next: Vec<String> = list.to_vec();
    match next.iter().position(|w| w.eq_ignore_ascii_case(word)) {
        Some(at) => {
            next.remove(at);
        }
        None => next.push(word.to_string()),
    }
    next
}

/// The panel for narrowing the results: type, explicit content and genres.
#[component]
fn FilterPanel(pending: usize, on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let filters = state.search_filters.read().clone();
    let empty = filters.is_empty();
    rsx! {
        div { class: "popover filter-panel",
            div { class: "popover-head",
                strong { class: "grow", "Narrow the results" }
                if pending > 0 { span { class: "muted small", "checking {pending}…" } }
                button { class: "btn small ghost", r#type: "button", disabled: empty,
                    onclick: move |_| set_filters(state, SearchFilters::default()),
                    "Clear"
                }
            }
            div { class: "filter-group",
                span { class: "filter-label", "Type" }
                div { class: "filter-pills",
                    for t in SERIES_TYPES {
                        {
                            let next = SearchFilters { types: toggled(&filters.types, t), ..filters.clone() };
                            let on = filters.types.iter().any(|x| x.eq_ignore_ascii_case(t));
                            rsx! {
                                button { key: "{t}", r#type: "button", class: if on { "pill small on" } else { "pill small" },
                                    onclick: move |_| set_filters(state, next.clone()),
                                    "{t}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "filter-group",
                span { class: "filter-label", "Explicit content" }
                div { class: "filter-pills",
                    for (choice, label) in [(ExplicitFilter::Show, "Show"), (ExplicitFilter::Hide, "Hide"), (ExplicitFilter::Only, "Only")] {
                        {
                            let next = SearchFilters { explicit: choice, ..filters.clone() };
                            rsx! {
                                button { key: "{label}", r#type: "button", class: if filters.explicit == choice { "pill small on" } else { "pill small" },
                                    onclick: move |_| set_filters(state, next.clone()),
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "filter-group",
                span { class: "filter-label", "Genres" }
                div { class: "filter-pills",
                    for g in COMMON_GENRES {
                        {
                            let next = SearchFilters { genres: toggled(&filters.genres, g), ..filters.clone() };
                            let on = filters.genres.iter().any(|x| x.eq_ignore_ascii_case(g));
                            rsx! {
                                button { key: "{g}", r#type: "button", class: if on { "pill small on" } else { "pill small" },
                                    onclick: move |_| set_filters(state, next.clone()),
                                    "{g}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "row", style: "margin-top: 12px",
                span { class: "muted small grow", "Types and genres are read from each series' details." }
                button { class: "btn primary small", r#type: "button", onclick: move |_| on_close.call(()), "Done" }
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
    let for_save = series.clone();
    let in_library = state
        .library
        .read()
        .find(&library::series_id(&series.title))
        .is_some();
    let save_class = if in_library {
        "btn small ghost add-btn on"
    } else {
        "btn small add-btn"
    };
    let save_title = if in_library {
        "It is in your library"
    } else {
        "Keep it in your library without opening it"
    };
    let save_icon = if in_library {
        "bookmark-check"
    } else {
        "bookmark-plus"
    };
    let save_text = if in_library {
        "In library"
    } else {
        "Add to library"
    };
    rsx! {
        div { class: "series-wrap",
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
        button {
            class: save_class,
            title: save_title,
            onclick: move |_| super::work::add_ranked_to_library(state, &for_save),
            Icon { name: save_icon, size: 15 }
            "{save_text}"
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
