//! `cinasafe import`: the app's import queue, from the command line.

use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::sync::Arc;

use cinasafe_core::importer::{Importer, ItemState, Lookup, MatchState, QueueItem};
use cinasafe_core::metadata::{tmdb::Tmdb, tvmaze::TvMaze, MetadataProvider, ReqwestClient};
use cinasafe_player::MpvProber;

use crate::library::ProviderChoice;

pub struct ImportArgs {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub online: bool,
    pub provider: ProviderChoice,
    pub tmdb_key: Option<String>,
    pub language: String,
    pub probe: bool,
}

fn lookup(a: &ImportArgs) -> Result<Lookup, String> {
    if !a.online {
        return Ok(Lookup::Offline);
    }
    let http = Arc::new(ReqwestClient::new().map_err(|e| e.to_string())?);
    let key = a.tmdb_key.as_deref().filter(|k| !k.trim().is_empty());
    let tvmaze: Arc<dyn MetadataProvider> = Arc::new(TvMaze::new(http.clone()));
    Ok(match (a.provider, key) {
        (ProviderChoice::Tvmaze, _) => Lookup::Online { movies: None, shows: tvmaze, http },
        (ProviderChoice::Tmdb, None) => return Err("--provider tmdb needs CINASAFE_TMDB_KEY or --tmdb-key".into()),
        (_, Some(k)) => {
            let t: Arc<dyn MetadataProvider> = Arc::new(Tmdb::new(http.clone(), k, &a.language));
            Lookup::Online { movies: Some(t.clone()), shows: t, http }
        }
        (ProviderChoice::Auto, None) => Lookup::Online { movies: None, shows: tvmaze, http },
    })
}

fn line(i: &QueueItem) -> Option<String> {
    let state = match &i.state {
        ItemState::Done => match i.match_state {
            MatchState::NeedsReview => "needs review".to_owned(),
            MatchState::NoMatch => "added (no match)".to_owned(),
            _ => "added".to_owned(),
        },
        ItemState::WontFit => "skipped: not enough space".into(),
        ItemState::AlreadyInVault => "skipped: already in the vault".into(),
        ItemState::NoVideo => "skipped: no video for this subtitle".into(),
        ItemState::Failed { message } => format!("FAILED: {message}"),
        ItemState::Cancelled => "cancelled".into(),
        _ => return None,
    };
    Some(format!("{:<28} {}  ←  {}", state, i.detected, i.file_name))
}

pub fn import(a: ImportArgs) -> Result<bool, String> {
    let vault = Arc::new(crate::vault_cmds::unlock(&a.root)?);
    if a.online {
        eprintln!("Online lookups are on: parsed titles are sent to the metadata provider.");
    }
    let tty = io::stderr().is_terminal();
    let v = vault.clone();
    let prober = a.probe.then(|| Arc::new(MpvProber::default()) as Arc<dyn cinasafe_core::importer::Prober>);
    let importer = Importer::new(
        Arc::new(move || Some(v.clone())),
        lookup(&a)?,
        prober,
        Arc::new(move |i: &QueueItem| {
            if let Some(l) = line(i) {
                if tty {
                    eprint!("\r\x1b[K");
                }
                println!("{l}");
            } else if tty {
                let pct = match i.state {
                    ItemState::Encrypting { fraction } => fraction * 50.0,
                    ItemState::Verifying { fraction } => 50.0 + fraction * 50.0,
                    _ => 0.0,
                };
                let eta = i.eta_seconds.map(|s| format!(", {}:{:02} left", s / 60, s % 60)).unwrap_or_default();
                eprint!("\r\x1b[K{}: {pct:.0}%{eta}", i.file_name);
            }
        }),
    );
    importer.add(&a.paths).map_err(|e| e.to_string())?;
    importer.wait_idle();
    let items = importer.items();
    let failed = items.iter().filter(|i| matches!(i.state, ItemState::Failed { .. })).count();
    let review: Vec<_> = items.iter().filter(|i| i.match_state == MatchState::NeedsReview).collect();
    let done = items.iter().filter(|i| i.state == ItemState::Done).count();
    eprintln!("{done} added, {} need review, {failed} failed.", review.len());
    for i in review {
        let names: Vec<String> = i
            .candidates
            .iter()
            .take(3)
            .map(|c| {
                format!("{} ({})", c.candidate.title, c.candidate.year.map(|y| y.to_string()).unwrap_or("?".into()))
            })
            .collect();
        eprintln!("  {}: {}", i.file_name, names.join(" / "));
    }
    Ok(failed == 0)
}
