//! A screen as one self-contained HTML file: no server, no script, the web
//! view's stylesheet inlined. For a report to keep or send.

use pma_core::screen::{Body, Cell, Column, Kind, Pane, Screen};

use crate::STYLE;

/// Overrides for a page that is read, not operated: tables run to their
/// full length and rows do not look clickable.
const PRINT: &str = "
.report { max-width: 1280px; margin: 0 auto; }
.report .table-wrap { max-height: none; }
.report tbody tr { cursor: default; }
.report .topbar { padding: 24px 28px 0; background: none; border: 0; }
.report footer { color: var(--text-3); font-size: 12px; padding: 0 28px 28px; }
@media print { .card { break-inside: avoid; box-shadow: none; } }
";

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Escaped text with `code` spans, as the web view draws them.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let Some(len) = rest[start + 1..].find('`') else {
            break;
        };
        out.push_str(&escape(&rest[..start]));
        out.push_str(&format!(
            "<code>{}</code>",
            escape(&rest[start + 1..start + 1 + len])
        ));
        rest = &rest[start + 2 + len..];
    }
    out.push_str(&escape(rest));
    out
}

fn cell(c: &Cell, col: &Column) -> String {
    let shown = shown(c, col);
    match &c.link {
        Some(url) => format!(
            r#"<a class="out" href="{}" target="_blank" rel="noopener noreferrer">{shown}</a>"#,
            escape(url)
        ),
        None => shown,
    }
}

fn shown(c: &Cell, col: &Column) -> String {
    match col.kind {
        Kind::Badge if !c.text.is_empty() => format!(
            r#"<span class="pill tone-{}">{}</span>"#,
            c.tone.name(),
            escape(&c.text)
        ),
        _ => format!(
            r#"<span class="tone-{}">{}</span>"#,
            c.tone.name(),
            inline(&c.text)
        ),
    }
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Text => "text",
        Kind::Wide => "wide",
        Kind::Number => "number",
        Kind::Badge => "badge",
    }
}

fn pane(p: &Pane) -> String {
    let mut out = format!(
        r#"<section class="card"><header class="card-head"><h2>{}</h2><span class="count">{}</span></header>"#,
        escape(&p.title),
        p.items.len()
    );
    if p.items.is_empty() {
        out.push_str(&format!(
            r#"<p class="empty">{}</p></section>"#,
            inline(p.empty)
        ));
        return out;
    }
    // A column no row fills says nothing; it is left out, as on the web.
    let shown: Vec<usize> = (0..p.columns.len())
        .filter(|&i| {
            p.items
                .iter()
                .any(|it| it.cells.get(i).is_some_and(|c| !c.text.is_empty()))
        })
        .collect();
    out.push_str(r#"<div class="table-wrap"><table><thead><tr>"#);
    for &i in &shown {
        let c = &p.columns[i];
        out.push_str(&format!(
            r#"<th scope="col" class="{}">{}</th>"#,
            kind(c.kind),
            escape(c.title)
        ));
    }
    out.push_str("</tr></thead><tbody>");
    for item in &p.items {
        out.push_str("<tr>");
        for &i in &shown {
            let col = &p.columns[i];
            let text = item.cells.get(i).map_or(String::new(), |c| cell(c, col));
            out.push_str(&format!(r#"<td class="{}">{text}</td>"#, kind(col.kind)));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div></section>");
    out
}

/// A tree as nested lists, every branch a `<details>` open as its node says,
/// so a static page still opens and closes without a script.
fn tree(nodes: &[pma_core::screen::Node], out: &mut String) {
    out.push_str(r#"<ul class="tree-list">"#);
    for n in nodes {
        let mut line = format!(
            r#"<span class="icon tone-{}">{}</span> <strong class="tone-{}">{}</strong>"#,
            n.icon.tone.name(),
            escape(&n.icon.text),
            n.label.tone.name(),
            inline(&n.label.text)
        );
        for f in n
            .facts
            .iter()
            .chain(&n.cells)
            .filter(|f| !f.text.is_empty())
        {
            line.push_str(&format!(
                r#" <span class="fact tone-{}">{}</span>"#,
                f.tone.name(),
                inline(&f.text)
            ));
        }
        match n.children.is_empty() {
            true => out.push_str(&format!("<li>{line}</li>")),
            false => {
                out.push_str(&format!(
                    "<li><details{}><summary>{line}</summary>",
                    if n.open { " open" } else { "" }
                ));
                tree(&n.children, out);
                out.push_str("</details></li>");
            }
        }
    }
    out.push_str("</ul>");
}

/// `screen` as a page titled `title`, stamped `generated`.
pub fn page(screen: &Screen, title: &str, generated: &str) -> String {
    let mut body = String::new();
    if !screen.summary.is_empty() {
        body.push_str(r#"<dl class="stats">"#);
        for (k, c) in &screen.summary {
            body.push_str(&format!(
                r#"<div class="stat"><dt>{}</dt><dd class="tone-{}">{}</dd></div>"#,
                escape(k),
                c.tone.name(),
                inline(&c.text)
            ));
        }
        body.push_str("</dl>");
    }
    match &screen.body {
        Body::Panes(panes) => {
            for p in panes {
                body.push_str(&pane(p));
            }
        }
        Body::Tree(t) => {
            body.push_str(r#"<section class="card tree">"#);
            tree(&t.nodes, &mut body);
            body.push_str("</section>");
        }
        Body::Detail(d) => {
            body.push_str(&format!(
                r#"<section class="card"><header class="card-head"><h2>{}</h2></header><div class="drawer-body">"#,
                escape(&d.title)
            ));
            if let Some(n) = &d.notice {
                body.push_str(&format!(r#"<p class="notice">{}</p>"#, inline(n)));
            }
            if !d.fields.is_empty() {
                body.push_str(r#"<dl class="fields">"#);
                for (k, v) in &d.fields {
                    body.push_str(&format!("<dt>{}</dt><dd>{}</dd>", escape(k), inline(v)));
                }
                body.push_str("</dl>");
            }
            for (name, _, text) in &d.sections {
                body.push_str(&format!(
                    r#"<div class="section"><h3>{}</h3><pre class="log">{}</pre></div>"#,
                    escape(name),
                    escape(text)
                ));
            }
            body.push_str("</div></section>");
        }
    }
    let header = screen.header.as_deref().unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>{title}</title>
<style>{STYLE}{PRINT}</style>
</head>
<body>
<main class="report">
<header class="topbar"><div class="heading"><h1>{title}</h1><p>{header}</p></div></header>
<div class="content">{body}</div>
<footer>Generated by pma {version}, {generated}.</footer>
</main>
</body>
</html>
"#,
        title = escape(title),
        header = escape(header),
        version = env!("CARGO_PKG_VERSION"),
        generated = escape(generated),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pma_core::screen::{Item, Pick, Tone};

    fn text(t: &str, tone: Tone) -> Cell {
        Cell {
            text: t.into(),
            tone,
            link: None,
        }
    }

    #[test]
    fn a_pane_is_a_table_with_its_tones_and_escaped_text() {
        let screen = Screen {
            tab: None,
            header: Some("last scan <now>".into()),
            summary: Vec::new(),
            body: Body::Panes(vec![
                Pane {
                    title: "Projects".into(),
                    columns: vec![
                        Column {
                            title: "project",
                            kind: Kind::Text,
                        },
                        Column {
                            title: "ci",
                            kind: Kind::Badge,
                        },
                        Column {
                            title: "never",
                            kind: Kind::Text,
                        },
                    ],
                    items: vec![Item {
                        cells: vec![
                            Cell {
                                link: Some("https://github.com/o/r?a=1&b=\"".into()),
                                ..text("a<b>&`x`", Tone::Plain)
                            },
                            text("failing", Tone::Bad),
                            text("", Tone::Plain),
                        ],
                        pick: Pick::Nothing,
                    }],
                    empty: "none",
                },
                Pane {
                    title: "Empty".into(),
                    columns: Vec::new(),
                    items: Vec::new(),
                    empty: "Run `pma scan`.",
                },
            ]),
        };
        let html = page(&screen, "pma status", "2026-10-20");
        assert!(html.contains("<title>pma status</title>"));
        assert!(html.contains("last scan &lt;now&gt;"));
        assert!(html.contains("a&lt;b&gt;&amp;<code>x</code>"), "{html}");
        assert!(
            html.contains(
                r#"<a class="out" href="https://github.com/o/r?a=1&amp;b=&quot;" target="_blank""#
            ),
            "{html}"
        );
        assert!(html.contains(r#"<span class="pill tone-bad">failing</span>"#));
        assert!(!html.contains(">never<"), "an empty column is left out");
        assert!(html.contains("Run <code>pma scan</code>."));
        assert!(!html.contains("<script"), "the page needs no script");
    }

    #[test]
    fn an_unpaired_backtick_stays_text() {
        assert_eq!(inline("a `b"), "a `b");
        assert_eq!(inline("`a` b `c"), "<code>a</code> b `c");
    }
}
