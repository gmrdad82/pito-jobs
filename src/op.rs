use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::util::Clock;

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Event(Event),
    End(End),
    Summary(Summary),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Start,
    Progress,
    Done,
    Fail,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub app: String,
    #[serde(rename = "ref", default)]
    pub reference: Option<String>,
    pub stage: String,
    pub state: State,
    #[serde(default)]
    pub fraction: Option<f64>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct End {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub app: String,
    #[serde(rename = "ref", default)]
    pub reference: Option<String>,
    pub ok: bool,
    #[serde(default)]
    pub msg: String,
    #[serde(default)]
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    #[serde(default)]
    pub op: String,
    pub ok: bool,
    #[serde(default)]
    pub results: Vec<Outcome>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub app: String,
    #[serde(rename = "ref", default)]
    pub reference: Option<String>,
    pub ok: bool,
    #[serde(default)]
    pub msg: String,
}

#[derive(Serialize)]
struct Tagged<'a, T: Serialize> {
    v: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    #[serde(flatten)]
    body: &'a T,
}

fn tagged<S: serde::Serializer, T: Serialize>(
    serializer: S,
    state: Option<&'static str>,
    body: &T,
) -> Result<S::Ok, S::Error> {
    Tagged { v: 1, state, body }.serialize(serializer)
}

impl Serialize for Line {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Line::Event(event) => tagged(serializer, None, event),
            Line::End(end) => tagged(serializer, Some("end"), end),
            Line::Summary(summary) => tagged(serializer, Some("summary"), summary),
        }
    }
}

pub(crate) fn clean(fraction: Option<f64>) -> Option<f64> {
    fraction
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 1.0))
}

impl Line {
    pub fn parse(text: &str) -> Option<Line> {
        Line::parse_bytes(text.as_bytes())
    }

    pub fn parse_bytes(bytes: &[u8]) -> Option<Line> {
        let Ok(Value::Object(mut map)) = serde_json::from_slice::<Value>(bytes) else {
            return None;
        };
        if map.remove("v").and_then(|v| v.as_u64()) != Some(1) {
            return None;
        }
        let state = map.get("state").and_then(Value::as_str)?;
        let (end, summary) = (state == "end", state == "summary");
        if end || summary {
            map.remove("state");
        }
        let body = Value::Object(map);
        if end {
            serde_json::from_value(body).ok().map(Line::End)
        } else if summary {
            serde_json::from_value(body).ok().map(Line::Summary)
        } else {
            serde_json::from_value::<Event>(body).ok().map(|mut event| {
                event.fraction = clean(event.fraction);
                Line::Event(event)
            })
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

pub fn read(input: impl BufRead) -> impl Iterator<Item = Line> {
    input
        .split(b'\n')
        .map_while(Result::ok)
        .filter_map(|bytes| Line::parse_bytes(&bytes))
}

pub struct Writer<W: Write> {
    out: W,
    op: String,
    app: String,
    reference: Option<String>,
    id: Option<String>,
    ended: bool,
    clock: Clock,
}

impl<W: Write> Writer<W> {
    pub fn new(out: W, op: &str, app: &str) -> Writer<W> {
        Writer::with_clock(out, op, app, Clock::system())
    }

    pub(crate) fn with_clock(out: W, op: &str, app: &str, clock: Clock) -> Writer<W> {
        Writer {
            out,
            op: op.to_string(),
            app: app.to_string(),
            reference: None,
            id: None,
            ended: false,
            clock,
        }
    }

    pub fn reference(&mut self, reference: &str) {
        self.reference = Some(reference.to_string());
    }

    pub fn id(&mut self, id: &str) {
        self.id = Some(id.to_string());
    }

    pub fn event(
        &mut self,
        stage: &str,
        state: State,
        fraction: Option<f64>,
        msg: Option<&str>,
    ) -> io::Result<()> {
        if self.ended {
            return Ok(());
        }
        let line = Line::Event(Event {
            op: self.op.clone(),
            app: self.app.clone(),
            reference: self.reference.clone(),
            stage: stage.to_string(),
            state,
            fraction: clean(fraction).map(|value| (value * 1000.0).round() / 1000.0),
            msg: msg.map(str::to_string),
            at: self.clock.wall_ms(),
            id: self.id.clone(),
        });
        self.line(&line)
    }

    pub fn end(&mut self, ok: bool, msg: &str) -> io::Result<()> {
        if self.ended {
            return Ok(());
        }
        let line = Line::End(End {
            op: self.op.clone(),
            app: self.app.clone(),
            reference: self.reference.clone(),
            ok,
            msg: msg.to_string(),
            at: self.clock.wall_ms(),
            id: self.id.clone(),
            extra: Map::new(),
        });
        self.line(&line)
    }

    pub fn line(&mut self, line: &Line) -> io::Result<()> {
        if matches!(line, Line::End(_)) {
            self.ended = true;
        }
        let mut bytes = line.to_json().into_bytes();
        bytes.push(b'\n');
        self.out.write_all(&bytes)?;
        self.out.flush()
    }
}
