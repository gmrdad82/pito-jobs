use super::record::Record;

pub(crate) const LIMIT: usize = 4096;
const STORE_CAP: usize = 4096;
pub(crate) const MAX_STAGES: usize = 16;

pub(crate) fn cap(text: String) -> String {
    if text.len() <= STORE_CAP {
        return text;
    }
    let mut text = text;
    let keep = text.floor_char_boundary(STORE_CAP);
    text.truncate(keep);
    text
}

pub(crate) fn encode(record: &Record) -> Vec<u8> {
    let Ok(bytes) = serde_json::to_vec(record) else {
        return Vec::new();
    };
    if bytes.len() <= LIMIT {
        return bytes;
    }
    let mut copy = record.clone();
    fit(&mut copy);
    match serde_json::to_vec(&copy) {
        Ok(bytes) if bytes.len() <= LIMIT => bytes,
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy)]
enum Which {
    Label,
    Detail,
    StageNote,
    Error,
    Device,
    Out,
    App,
    Invoker,
    Parent,
    Tool,
    Host,
    Id,
    Stage,
    Unit,
    AsName,
}

const PREFERRED: [Which; 4] = [Which::Label, Which::Detail, Which::StageNote, Which::Error];

const EXTRA: [Which; 11] = [
    Which::Device,
    Which::Out,
    Which::App,
    Which::Invoker,
    Which::Parent,
    Which::Tool,
    Which::AsName,
    Which::Stage,
    Which::Unit,
    Which::Host,
    Which::Id,
];

fn fit(record: &mut Record) {
    for _ in 0..24 {
        let Ok(bytes) = serde_json::to_vec(record) else {
            return;
        };
        if bytes.len() <= LIMIT {
            return;
        }
        let over = bytes.len() - LIMIT;
        if !record.stages.is_empty() {
            record.stages.clear();
            continue;
        }
        if shrink(record, &PREFERRED, over) || shrink(record, &EXTRA, over) {
            continue;
        }
        record.label.clear();
        record.detail = None;
        record.stage_note = None;
        record.error = None;
        record.device = None;
        record.out = None;
        return;
    }
}

fn shrink(record: &mut Record, group: &[Which], over: usize) -> bool {
    let mut best: Option<Which> = None;
    let mut best_len = 0;
    for which in group {
        let len = text(record, *which).map(str::len).unwrap_or(0);
        if len > best_len {
            best_len = len;
            best = Some(*which);
        }
    }
    let Some(which) = best else {
        return false;
    };
    let Some(text) = text_mut(record, which) else {
        return false;
    };
    shorten(text, over)
}

fn shorten(text: &mut String, over: usize) -> bool {
    let before = text.len();
    if before == 0 {
        return false;
    }
    let keep = text.floor_char_boundary(before.saturating_sub(over.max(1)));
    text.truncate(keep);
    text.len() < before
}

fn text(record: &Record, which: Which) -> Option<&str> {
    match which {
        Which::Label => Some(record.label.as_str()),
        Which::Detail => record.detail.as_deref(),
        Which::StageNote => record.stage_note.as_deref(),
        Which::Error => record.error.as_deref(),
        Which::Device => record.device.as_deref(),
        Which::Out => record.out.as_deref(),
        Which::App => record.app.as_deref(),
        Which::Invoker => record.invoker.as_deref(),
        Which::Parent => record.parent.as_deref(),
        Which::Tool => Some(record.tool.as_str()),
        Which::Host => Some(record.host.as_str()),
        Which::Id => Some(record.id.as_str()),
        Which::Stage => record.stage.as_deref(),
        Which::Unit => record
            .progress
            .as_ref()
            .map(|progress| progress.unit.as_str()),
        Which::AsName => record.pgpu.as_ref().map(|pgpu| pgpu.as_name.as_str()),
    }
}

fn text_mut(record: &mut Record, which: Which) -> Option<&mut String> {
    match which {
        Which::Label => Some(&mut record.label),
        Which::Detail => record.detail.as_mut(),
        Which::StageNote => record.stage_note.as_mut(),
        Which::Error => record.error.as_mut(),
        Which::Device => record.device.as_mut(),
        Which::Out => record.out.as_mut(),
        Which::App => record.app.as_mut(),
        Which::Invoker => record.invoker.as_mut(),
        Which::Parent => record.parent.as_mut(),
        Which::Tool => Some(&mut record.tool),
        Which::Host => Some(&mut record.host),
        Which::Id => Some(&mut record.id),
        Which::Stage => record.stage.as_mut(),
        Which::Unit => record.progress.as_mut().map(|progress| &mut progress.unit),
        Which::AsName => record.pgpu.as_mut().map(|pgpu| &mut pgpu.as_name),
    }
}
