#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    New,
    Open,
    OpenBuffer,
    Periscope,
    Lance,
    LanceAdd,
    LanceNext,
    LancePrev,
    FlowRescan,
    FlowRebuild,
    Daily,
    Refile,
    QuickAdd,
    Save,
    SaveAs,
    Scratch,
    ScratchEntries,
    Capture,
    TogglePreview,
    ToggleWrap,
    Settings,
    LineNumbersAbsolute,
    LineNumbersRelative,
    WrapOn,
    WrapOff,
    PreviewOn,
    PreviewOff,
    DocTasks,
    Agenda,
    Backlinks,
    Mentions,
    FindNote,
    OpenLink,
    Quit,
}

pub(crate) struct CommandSpec {
    pub(crate) name: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) summary: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) palette_command: Option<Command>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandUsage {
    pub(crate) name: String,
    pub(crate) count: usize,
    pub(crate) last_used: i64,
}

pub(crate) const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "save",
        aliases: &["w", "write"],
        summary: "Save current buffer",
        hint: "Ctrl+S",
        palette_command: Some(Command::Save),
    },
    CommandSpec {
        name: "quit",
        aliases: &["q", "exit"],
        summary: "Quit Slate",
        hint: "Ctrl+Q",
        palette_command: Some(Command::Quit),
    },
    CommandSpec {
        name: "wq",
        aliases: &["x"],
        summary: "Save and quit",
        hint: ":wq",
        palette_command: None,
    },
    CommandSpec {
        name: "new",
        aliases: &["enew"],
        summary: "New buffer",
        hint: "Ctrl+N",
        palette_command: Some(Command::New),
    },
    CommandSpec {
        name: "open",
        aliases: &["edit", "e"],
        summary: "Open file or path",
        hint: "Ctrl+O",
        palette_command: Some(Command::Open),
    },
    CommandSpec {
        name: "lance",
        aliases: &["lc", "marks"],
        summary: "Open Lance-style marked files",
        hint: ":lance",
        palette_command: Some(Command::Lance),
    },
    CommandSpec {
        name: "lance-add",
        aliases: &["la", "mark-buffer"],
        summary: "Add current file to Lance marks",
        hint: ":lance-add",
        palette_command: Some(Command::LanceAdd),
    },
    CommandSpec {
        name: "lance-next",
        aliases: &["lnext"],
        summary: "Open next Lance mark",
        hint: ":lance-next",
        palette_command: Some(Command::LanceNext),
    },
    CommandSpec {
        name: "lance-prev",
        aliases: &["lpv"],
        summary: "Open previous Lance mark",
        hint: ":lance-prev",
        palette_command: Some(Command::LancePrev),
    },
    CommandSpec {
        name: "open-buffer",
        aliases: &["ob", "buffer-open"],
        summary: "Open a file in the editable modal buffer",
        hint: ":open-buffer",
        palette_command: Some(Command::OpenBuffer),
    },
    CommandSpec {
        name: "periscope",
        aliases: &["ps", "fzf"],
        summary: "Open Periscope project/home file search",
        hint: ":periscope",
        palette_command: Some(Command::Periscope),
    },
    CommandSpec {
        name: "flow-rescan",
        aliases: &["flow-index"],
        summary: "Rescan current Flow scope into .slate and agenda cache",
        hint: ":flow-rescan",
        palette_command: Some(Command::FlowRescan),
    },
    CommandSpec {
        name: "flow-rebuild",
        aliases: &["flow-reindex"],
        summary: "Rebuild current Flow scope index and agenda cache",
        hint: ":flow-rebuild",
        palette_command: Some(Command::FlowRebuild),
    },
    CommandSpec {
        name: "daily",
        aliases: &["daily-log", "journal"],
        summary: "Open the monthly Daily reflection log",
        hint: ":daily [today|yesterday|YYYY-MM-DD|refresh]",
        palette_command: Some(Command::Daily),
    },
    CommandSpec {
        name: "refile",
        aliases: &["rf"],
        summary: "Copy current Flow line to another Markdown file and link both sides",
        hint: ":refile",
        palette_command: Some(Command::Refile),
    },
    CommandSpec {
        name: "quickadd",
        aliases: &["qa", "inbox"],
        summary: "Append a Slate Flow task to the current inbox",
        hint: ":quickadd comprar pan #casa ALTA",
        palette_command: Some(Command::QuickAdd),
    },
    CommandSpec {
        name: "open-last",
        aliases: &["last", "ol"],
        summary: "Open last file",
        hint: "Ctrl+O L",
        palette_command: None,
    },
    CommandSpec {
        name: "recent",
        aliases: &["rec"],
        summary: "Open recent files picker",
        hint: ":recent",
        palette_command: None,
    },
    CommandSpec {
        name: "scratch",
        aliases: &["sc"],
        summary: "Open quick scratch capture modal",
        hint: ":scratch",
        palette_command: Some(Command::Scratch),
    },
    CommandSpec {
        name: "capture",
        aliases: &["cap"],
        summary: "Capture selection or current line to scratch",
        hint: ":capture",
        palette_command: Some(Command::Capture),
    },
    CommandSpec {
        name: "scratch-entries",
        aliases: &["scratch-log", "scl"],
        summary: "Review and clean scratch archive entries",
        hint: ":scratch-entries",
        palette_command: Some(Command::ScratchEntries),
    },
    CommandSpec {
        name: "save-as",
        aliases: &["saveas", "write-as"],
        summary: "Save current buffer with a new path",
        hint: "Ctrl+Alt+S",
        palette_command: Some(Command::SaveAs),
    },
    CommandSpec {
        name: "preview",
        aliases: &["md"],
        summary: "Toggle Markdown preview",
        hint: "Ctrl+M",
        palette_command: Some(Command::TogglePreview),
    },
    CommandSpec {
        name: "wrap",
        aliases: &[],
        summary: "Toggle word wrap",
        hint: ":wrap",
        palette_command: Some(Command::ToggleWrap),
    },
    CommandSpec {
        name: "wrap-on",
        aliases: &["wrap-enable"],
        summary: "Enable word wrap",
        hint: ":wrap on",
        palette_command: Some(Command::WrapOn),
    },
    CommandSpec {
        name: "wrap-off",
        aliases: &["nowrap", "wrap-disable"],
        summary: "Disable word wrap",
        hint: ":wrap off",
        palette_command: Some(Command::WrapOff),
    },
    CommandSpec {
        name: "preview-on",
        aliases: &["md-on"],
        summary: "Enable Markdown preview",
        hint: ":preview on",
        palette_command: Some(Command::PreviewOn),
    },
    CommandSpec {
        name: "preview-off",
        aliases: &["md-off"],
        summary: "Disable Markdown preview",
        hint: ":preview off",
        palette_command: Some(Command::PreviewOff),
    },
    CommandSpec {
        name: "backlinks",
        aliases: &["bl"],
        summary: "Show Markdown links pointing to the current file",
        hint: ":backlinks",
        palette_command: Some(Command::Backlinks),
    },
    CommandSpec {
        name: "mentions",
        aliases: &["refs"],
        summary: "Show textual mentions of the current file",
        hint: ":mentions",
        palette_command: Some(Command::Mentions),
    },
    CommandSpec {
        name: "find-note",
        aliases: &["fn", "note-find"],
        summary: "Find Markdown notes by file, heading, or content",
        hint: ":find-note query",
        palette_command: Some(Command::FindNote),
    },
    CommandSpec {
        name: "open-link",
        aliases: &["olink", "follow-link"],
        summary: "Open Markdown link under cursor",
        hint: ":open-link",
        palette_command: Some(Command::OpenLink),
    },
    CommandSpec {
        name: "doc-tasks",
        aliases: &["tasks"],
        summary: "Browse current-document checkboxes",
        hint: ":doc-tasks",
        palette_command: Some(Command::DocTasks),
    },
    CommandSpec {
        name: "agenda",
        aliases: &["ag"],
        summary: "Open Slate Flow agenda",
        hint: ":agenda [#tag|@project|overdue]",
        palette_command: Some(Command::Agenda),
    },
    CommandSpec {
        name: "find",
        aliases: &["f"],
        summary: "Find text",
        hint: "Ctrl+F",
        palette_command: None,
    },
    CommandSpec {
        name: "goto",
        aliases: &["g", "line", "l"],
        summary: "Go to line/column",
        hint: ":g 10",
        palette_command: None,
    },
    CommandSpec {
        name: "select-word",
        aliases: &["sw"],
        summary: "Select word under cursor",
        hint: "Ctrl S W",
        palette_command: None,
    },
    CommandSpec {
        name: "select-line",
        aliases: &["sl"],
        summary: "Select current line",
        hint: "Ctrl S L",
        palette_command: None,
    },
    CommandSpec {
        name: "delete-word",
        aliases: &["dw"],
        summary: "Delete word under cursor",
        hint: "Ctrl D W",
        palette_command: None,
    },
    CommandSpec {
        name: "delete-line",
        aliases: &["dl"],
        summary: "Delete current line",
        hint: "Ctrl D L",
        palette_command: None,
    },
    CommandSpec {
        name: "duplicate-line",
        aliases: &["dup"],
        summary: "Duplicate current line",
        hint: "Ctrl D U P",
        palette_command: None,
    },
    CommandSpec {
        name: "duplicate-place",
        aliases: &["dupp"],
        summary: "Duplicate line then place it",
        hint: "Ctrl D U P P",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-up",
        aliases: &["mlu"],
        summary: "Move current line up",
        hint: "Alt+Up",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-down",
        aliases: &["mld"],
        summary: "Move current line down",
        hint: "Alt+Down",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-to-paragraph-start",
        aliases: &["mlps"],
        summary: "Move line to paragraph start",
        hint: "Alt double-up",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-to-paragraph-end",
        aliases: &["mlpe"],
        summary: "Move line to paragraph end",
        hint: "Alt double-down",
        palette_command: None,
    },
    CommandSpec {
        name: "top",
        aliases: &["go-top", "gt"],
        summary: "Go to top of file",
        hint: "Ctrl G T",
        palette_command: None,
    },
    CommandSpec {
        name: "bottom",
        aliases: &["go-bottom", "gb"],
        summary: "Go to bottom of file",
        hint: "Ctrl G B",
        palette_command: None,
    },
    CommandSpec {
        name: "line-numbers",
        aliases: &["ln", "linenumbers"],
        summary: "Set line number mode",
        hint: ":line-numbers relative",
        palette_command: None,
    },
    CommandSpec {
        name: "line-numbers-absolute",
        aliases: &["ln-abs"],
        summary: "Use absolute line numbers",
        hint: ":ln absolute",
        palette_command: Some(Command::LineNumbersAbsolute),
    },
    CommandSpec {
        name: "line-numbers-relative",
        aliases: &["ln-rel"],
        summary: "Use relative line numbers",
        hint: ":ln relative",
        palette_command: Some(Command::LineNumbersRelative),
    },
    CommandSpec {
        name: "settings",
        aliases: &["set", "prefs", "preferences"],
        summary: "Open settings",
        hint: ":settings",
        palette_command: Some(Command::Settings),
    },
];

pub(crate) fn clamp_history_limit(limit: usize) -> usize {
    limit.clamp(1, 50)
}

pub(crate) fn trim_command_history(history: &mut Vec<String>, limit: usize) {
    let limit = clamp_history_limit(limit);
    if history.len() > limit {
        let keep_from = history.len() - limit;
        history.drain(0..keep_from);
    }
}

pub(crate) fn push_command_history(history: &mut Vec<String>, input: &str, limit: usize) -> bool {
    if history.last().is_some_and(|last| last == input) {
        return false;
    }
    history.push(input.to_string());
    trim_command_history(history, limit);
    true
}

pub(crate) fn command_name(command: Command) -> &'static str {
    match command {
        Command::New => "new",
        Command::Open => "open",
        Command::OpenBuffer => "open-buffer",
        Command::Periscope => "periscope",
        Command::Lance => "lance",
        Command::LanceAdd => "lance-add",
        Command::LanceNext => "lance-next",
        Command::LancePrev => "lance-prev",
        Command::FlowRescan => "flow-rescan",
        Command::FlowRebuild => "flow-rebuild",
        Command::Daily => "daily",
        Command::Refile => "refile",
        Command::QuickAdd => "quickadd",
        Command::Save => "save",
        Command::SaveAs => "save-as",
        Command::Scratch => "scratch",
        Command::ScratchEntries => "scratch-entries",
        Command::Capture => "capture",
        Command::TogglePreview => "preview",
        Command::ToggleWrap => "wrap",
        Command::Settings => "settings",
        Command::LineNumbersAbsolute => "line-numbers-absolute",
        Command::LineNumbersRelative => "line-numbers-relative",
        Command::WrapOn => "wrap-on",
        Command::WrapOff => "wrap-off",
        Command::PreviewOn => "preview-on",
        Command::PreviewOff => "preview-off",
        Command::DocTasks => "doc-tasks",
        Command::Agenda => "agenda",
        Command::Backlinks => "backlinks",
        Command::Mentions => "mentions",
        Command::FindNote => "find-note",
        Command::OpenLink => "open-link",
        Command::Quit => "quit",
    }
}

pub(crate) fn canonical_command_name(token: &str) -> Option<&'static str> {
    let token = token.trim_start_matches(':');
    COMMAND_SPECS
        .iter()
        .find(|spec| spec.name == token || spec.aliases.contains(&token))
        .map(|spec| spec.name)
}

pub(crate) fn parse_command_usage(value: &str) -> Option<CommandUsage> {
    let mut parts = value.split('|');
    let name = canonical_command_name(parts.next()?)?;
    let count = parts.next()?.parse::<usize>().ok()?;
    let last_used = parts.next()?.parse::<i64>().ok()?;
    Some(CommandUsage {
        name: name.to_string(),
        count,
        last_used,
    })
}

pub(crate) fn merge_command_usage(usages: &mut Vec<CommandUsage>, incoming: CommandUsage) {
    if let Some(existing) = usages.iter_mut().find(|usage| usage.name == incoming.name) {
        existing.count = existing.count.max(incoming.count);
        existing.last_used = existing.last_used.max(incoming.last_used);
    } else {
        usages.push(incoming);
    }
}

pub(crate) fn sort_command_usage(usages: &mut Vec<CommandUsage>) {
    usages.sort_by_key(|usage| {
        (
            std::cmp::Reverse(usage.last_used),
            std::cmp::Reverse(usage.count),
            usage.name.clone(),
        )
    });
    usages.truncate(100);
}

pub(crate) fn record_command_usage(usages: &mut Vec<CommandUsage>, token: &str, now: i64) -> bool {
    let Some(name) = canonical_command_name(token) else {
        return false;
    };
    if let Some(usage) = usages.iter_mut().find(|usage| usage.name == name) {
        usage.count = usage.count.saturating_add(1);
        usage.last_used = now;
    } else {
        usages.push(CommandUsage {
            name: name.to_string(),
            count: 1,
            last_used: now,
        });
    }
    sort_command_usage(usages);
    true
}

pub(crate) fn command_usage_boost(usages: &[CommandUsage], name: &str, now: i64) -> usize {
    let Some(usage) = usages.iter().find(|usage| usage.name == name) else {
        return 0;
    };
    let count_boost = usage.count.min(30);
    let age_seconds = now.saturating_sub(usage.last_used);
    let recency_boost = if age_seconds <= 60 * 60 {
        30
    } else if age_seconds <= 24 * 60 * 60 {
        20
    } else if age_seconds <= 7 * 24 * 60 * 60 {
        10
    } else {
        0
    };
    count_boost + recency_boost
}

pub(crate) fn command_prefix(input: &str) -> Option<&str> {
    let input = input
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(input.trim_start());
    if input.is_empty() || input.contains(char::is_whitespace) {
        return None;
    }
    Some(input)
}

pub(crate) fn matching_command_specs(
    prefix: &str,
    limit: usize,
    usage_boost: impl Fn(&str) -> usize,
) -> Vec<&'static CommandSpec> {
    let query = prefix.trim_start_matches(':').to_lowercase();
    if query.is_empty() {
        let mut specs = COMMAND_SPECS.iter().collect::<Vec<_>>();
        specs.sort_by_key(|spec| {
            let boost = usage_boost(spec.name);
            (std::cmp::Reverse(boost), spec.name.len(), spec.name)
        });
        return specs.into_iter().take(limit).collect();
    }

    let mut scored = COMMAND_SPECS
        .iter()
        .filter_map(|spec| command_spec_score(spec, &query).map(|score| (score, spec)))
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, spec)| {
        let boost = usage_boost(spec.name);
        let adjusted_score = (*score as isize * 100) - boost as isize;
        (*score / 100, adjusted_score, spec.name.len(), spec.name)
    });
    scored
        .into_iter()
        .map(|(_, spec)| spec)
        .take(limit)
        .collect()
}

pub(crate) fn command_spec_score(spec: &CommandSpec, query: &str) -> Option<usize> {
    let mut best = fuzzy_score(spec.name, query);
    for alias in spec.aliases {
        best = best.into_iter().chain(fuzzy_score(alias, query)).min();
    }
    best
}

pub(crate) fn best_command_token(spec: &CommandSpec, prefix: &str) -> Option<&'static str> {
    let query = prefix.to_lowercase();
    std::iter::once(spec.name)
        .chain(spec.aliases.iter().copied())
        .filter_map(|token| fuzzy_score(token, &query).map(|score| (score, token)))
        .min_by_key(|(score, token)| (*score, token.len()))
        .map(|(_, token)| token)
}

pub(crate) fn fuzzy_score_lower(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    if candidate == query {
        return Some(0);
    }
    if candidate.starts_with(query) {
        return Some(1 + candidate.len().saturating_sub(query.len()));
    }
    if candidate.contains(query) {
        return Some(100 + candidate.find(query).unwrap_or(0));
    }

    let mut score = 200usize;
    let mut last_match = None;
    let mut chars = candidate.char_indices();
    for query_ch in query.chars() {
        let Some((index, _)) = chars.find(|(_, candidate_ch)| *candidate_ch == query_ch) else {
            return None;
        };
        if let Some(last) = last_match {
            score += index.saturating_sub(last + 1);
        }
        last_match = Some(index);
    }
    Some(score + candidate.len().saturating_sub(query.len()))
}

pub(crate) fn fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_lowercase();
    fuzzy_score_lower(&candidate, query)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommandLineNumberMode {
    Absolute,
    Relative,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommandLineAction {
    Empty,
    Run(Command),
    SaveQuit,
    OpenPath(String),
    OpenBufferPath(String),
    QuickAdd(Option<String>),
    Daily {
        target: String,
    },
    LanceSlot(usize),
    LanceAddPath(String),
    OpenLast,
    Recent {
        query: String,
    },
    SetPreview(Option<bool>),
    Agenda {
        filter: String,
    },
    FindNote {
        query: String,
    },
    SetWrap(Option<bool>),
    Find {
        query: String,
    },
    Goto {
        target: String,
    },
    SelectWord,
    SelectLine,
    DeleteWord,
    DeleteLine,
    DuplicateLine,
    DuplicatePlace,
    MoveLineUp,
    MoveLineDown,
    MoveLineToParagraphStart,
    MoveLineToParagraphEnd,
    Top,
    Bottom,
    LineNumbers {
        mode: Option<CommandLineNumberMode>,
        had_arg: bool,
    },
    Unknown(String),
}

pub(crate) fn parse_command_line(raw: &str) -> CommandLineAction {
    let input = raw.strip_prefix(':').unwrap_or(raw).trim();
    if input.is_empty() {
        return CommandLineAction::Empty;
    }

    let mut parts = input.split_whitespace();
    let Some(command) = parts.next() else {
        return CommandLineAction::Empty;
    };

    match command {
        "w" | "write" | "save" => CommandLineAction::Run(Command::Save),
        "save-as" | "saveas" | "write-as" => CommandLineAction::Run(Command::SaveAs),
        "scratch" | "sc" => CommandLineAction::Run(Command::Scratch),
        "scratch-entries" | "scratch-log" | "scl" => {
            CommandLineAction::Run(Command::ScratchEntries)
        }
        "capture" | "cap" => CommandLineAction::Run(Command::Capture),
        "q" | "quit" | "exit" => CommandLineAction::Run(Command::Quit),
        "wq" | "x" => CommandLineAction::SaveQuit,
        "new" | "enew" => CommandLineAction::Run(Command::New),
        "open" | "edit" | "e" => {
            let path = remaining_args(parts);
            if path.is_empty() {
                CommandLineAction::Run(Command::Open)
            } else {
                CommandLineAction::OpenPath(path)
            }
        }
        "open-buffer" | "ob" | "buffer-open" => {
            let path = remaining_args(parts);
            if path.is_empty() {
                CommandLineAction::Run(Command::OpenBuffer)
            } else {
                CommandLineAction::OpenBufferPath(path)
            }
        }
        "periscope" | "ps" | "fzf" => CommandLineAction::Run(Command::Periscope),
        "flow-rescan" | "flow-index" => CommandLineAction::Run(Command::FlowRescan),
        "flow-rebuild" | "flow-reindex" => CommandLineAction::Run(Command::FlowRebuild),
        "daily" | "daily-log" | "journal" => CommandLineAction::Daily {
            target: remaining_args(parts),
        },
        "refile" | "rf" => CommandLineAction::Run(Command::Refile),
        "quickadd" | "qa" | "inbox" => {
            let text = remaining_args(parts);
            CommandLineAction::QuickAdd((!text.trim().is_empty()).then_some(text))
        }
        "lance" | "lc" | "marks" => {
            let arg = parts.next();
            if let Some(slot) = arg.and_then(|value| value.parse::<usize>().ok()) {
                CommandLineAction::LanceSlot(slot)
            } else {
                CommandLineAction::Run(Command::Lance)
            }
        }
        "lance-add" | "la" | "mark-buffer" => {
            let path = remaining_args(parts);
            if path.is_empty() {
                CommandLineAction::Run(Command::LanceAdd)
            } else {
                CommandLineAction::LanceAddPath(path)
            }
        }
        "lance-next" | "lnext" => CommandLineAction::Run(Command::LanceNext),
        "lance-prev" | "lpv" => CommandLineAction::Run(Command::LancePrev),
        "open-last" | "last" | "ol" => CommandLineAction::OpenLast,
        "recent" | "rec" => CommandLineAction::Recent {
            query: remaining_args(parts),
        },
        "preview" | "md" => {
            CommandLineAction::SetPreview(parts.next().and_then(parse_command_bool))
        }
        "preview-on" | "md-on" => CommandLineAction::Run(Command::PreviewOn),
        "preview-off" | "md-off" => CommandLineAction::Run(Command::PreviewOff),
        "doc-tasks" | "tasks" => CommandLineAction::Run(Command::DocTasks),
        "agenda" | "ag" => CommandLineAction::Agenda {
            filter: remaining_args(parts),
        },
        "backlinks" | "bl" => CommandLineAction::Run(Command::Backlinks),
        "mentions" | "refs" => CommandLineAction::Run(Command::Mentions),
        "find-note" | "fn" | "note-find" => CommandLineAction::FindNote {
            query: remaining_args(parts),
        },
        "open-link" | "olink" | "follow-link" => CommandLineAction::Run(Command::OpenLink),
        "wrap" => CommandLineAction::SetWrap(parts.next().and_then(parse_command_bool)),
        "wrap-on" | "wrap-enable" => CommandLineAction::Run(Command::WrapOn),
        "wrap-off" | "nowrap" | "wrap-disable" => CommandLineAction::Run(Command::WrapOff),
        "find" | "f" => CommandLineAction::Find {
            query: remaining_args(parts),
        },
        "goto" | "g" | "line" | "l" => CommandLineAction::Goto {
            target: remaining_args(parts),
        },
        "select-word" | "sw" => CommandLineAction::SelectWord,
        "select-line" | "sl" => CommandLineAction::SelectLine,
        "delete-word" | "dw" => CommandLineAction::DeleteWord,
        "delete-line" | "dl" => CommandLineAction::DeleteLine,
        "duplicate-line" | "dup" => CommandLineAction::DuplicateLine,
        "duplicate-place" | "dupp" => CommandLineAction::DuplicatePlace,
        "move-line-up" | "mlu" => CommandLineAction::MoveLineUp,
        "move-line-down" | "mld" => CommandLineAction::MoveLineDown,
        "move-line-to-paragraph-start" | "mlps" => CommandLineAction::MoveLineToParagraphStart,
        "move-line-to-paragraph-end" | "mlpe" => CommandLineAction::MoveLineToParagraphEnd,
        "top" | "go-top" | "gt" => CommandLineAction::Top,
        "bottom" | "go-bottom" | "gb" => CommandLineAction::Bottom,
        "line-numbers" | "ln" | "linenumbers" => {
            let arg = parts.next();
            CommandLineAction::LineNumbers {
                mode: arg.and_then(parse_line_number_mode),
                had_arg: arg.is_some(),
            }
        }
        "line-numbers-absolute" | "ln-abs" => CommandLineAction::Run(Command::LineNumbersAbsolute),
        "line-numbers-relative" | "ln-rel" => CommandLineAction::Run(Command::LineNumbersRelative),
        "settings" | "set" | "prefs" | "preferences" => CommandLineAction::Run(Command::Settings),
        _ => CommandLineAction::Unknown(input.to_string()),
    }
}

fn remaining_args<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts.collect::<Vec<_>>().join(" ")
}

fn parse_command_bool(value: &str) -> Option<bool> {
    match value.trim().trim_matches('"').to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn parse_line_number_mode(value: &str) -> Option<CommandLineNumberMode> {
    match value.trim().trim_matches('"').to_lowercase().as_str() {
        "absolute" | "abs" => Some(CommandLineNumberMode::Absolute),
        "relative" | "rel" => Some(CommandLineNumberMode::Relative),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_prefix_accepts_colon_and_plain_command_tokens() {
        assert_eq!(command_prefix(":save"), Some("save"));
        assert_eq!(command_prefix("  open"), Some("open"));
        assert_eq!(command_prefix(":open file.md"), None);
        assert_eq!(command_prefix(":"), None);
    }

    #[test]
    fn canonical_command_name_resolves_aliases() {
        assert_eq!(canonical_command_name("w"), Some("save"));
        assert_eq!(canonical_command_name(":ag"), Some("agenda"));
        assert_eq!(canonical_command_name("missing"), None);
    }

    #[test]
    fn matching_specs_prefers_usage_for_empty_query() {
        let specs = matching_command_specs("", 1, |name| if name == "agenda" { 100 } else { 0 });
        assert_eq!(specs.first().map(|spec| spec.name), Some("agenda"));
    }

    #[test]
    fn best_token_can_complete_aliases() {
        let spec = COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == "agenda")
            .unwrap();
        assert_eq!(best_command_token(spec, "ag"), Some("ag"));
        assert_eq!(best_command_token(spec, "agen"), Some("agenda"));
    }

    #[test]
    fn parses_command_line_actions_with_arguments() {
        assert_eq!(parse_command_line(":"), CommandLineAction::Empty);
        assert_eq!(
            parse_command_line(":w"),
            CommandLineAction::Run(Command::Save)
        );
        assert_eq!(parse_command_line(":wq"), CommandLineAction::SaveQuit);
        assert_eq!(
            parse_command_line(":open docs/read me.md"),
            CommandLineAction::OpenPath("docs/read me.md".to_string())
        );
        assert_eq!(
            parse_command_line(":quickadd comprar pan #task"),
            CommandLineAction::QuickAdd(Some("comprar pan #task".to_string()))
        );
        assert_eq!(
            parse_command_line(":quickadd"),
            CommandLineAction::QuickAdd(None)
        );
        assert_eq!(
            parse_command_line(":lance 3"),
            CommandLineAction::LanceSlot(3)
        );
        assert_eq!(
            parse_command_line(":agenda #task @casa"),
            CommandLineAction::Agenda {
                filter: "#task @casa".to_string()
            }
        );
    }

    #[test]
    fn parses_boolean_and_line_number_command_arguments() {
        assert_eq!(
            parse_command_line(":wrap on"),
            CommandLineAction::SetWrap(Some(true))
        );
        assert_eq!(
            parse_command_line(":preview off"),
            CommandLineAction::SetPreview(Some(false))
        );
        assert_eq!(
            parse_command_line(":wrap maybe"),
            CommandLineAction::SetWrap(None)
        );
        assert_eq!(
            parse_command_line(":line-numbers relative"),
            CommandLineAction::LineNumbers {
                mode: Some(CommandLineNumberMode::Relative),
                had_arg: true
            }
        );
        assert_eq!(
            parse_command_line(":line-numbers weird"),
            CommandLineAction::LineNumbers {
                mode: None,
                had_arg: true
            }
        );
    }

    #[test]
    fn command_history_pushes_unique_recent_entries_with_limit() {
        let mut history = Vec::new();
        assert!(push_command_history(&mut history, "one", 2));
        assert!(!push_command_history(&mut history, "one", 2));
        assert!(push_command_history(&mut history, "two", 2));
        assert!(push_command_history(&mut history, "three", 2));
        assert_eq!(history, vec!["two".to_string(), "three".to_string()]);
    }

    #[test]
    fn command_usage_records_merges_and_scores() {
        let mut usages = Vec::new();
        assert!(record_command_usage(&mut usages, "w", 100));
        assert!(record_command_usage(&mut usages, "save", 120));
        assert_eq!(usages[0].name, "save");
        assert_eq!(usages[0].count, 2);
        assert_eq!(usages[0].last_used, 120);
        assert!(command_usage_boost(&usages, "save", 120) > 0);

        let parsed = parse_command_usage("ag|3|90").unwrap();
        merge_command_usage(&mut usages, parsed);
        assert!(
            usages
                .iter()
                .any(|usage| usage.name == "agenda" && usage.count == 3)
        );
    }
}
