use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
};

/// Maximum number of log lines to keep in memory.
const DEFAULT_MAX_LINES: usize = 10_000;

/// A log viewer that maintains a buffer of log lines with search capability.
#[derive(Debug)]
pub struct LogViewer {
    /// The log lines buffer (ring buffer behavior when full).
    lines: VecDeque<String>,
    /// Maximum number of lines to keep.
    max_lines: usize,
    /// Current scroll position (line index).
    scroll: usize,
    /// Whether to auto-scroll to new lines.
    follow: bool,
    /// Current search query.
    search_query: Option<String>,
    /// Indices of lines matching the search query.
    search_matches: Vec<usize>,
    /// Current search match index.
    search_index: usize,
    /// Path to the currently watched log file.
    log_path: Option<PathBuf>,
    /// Last known file position for incremental reading.
    last_position: u64,
}

impl Default for LogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogViewer {
    /// Create a new log viewer.
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            max_lines: DEFAULT_MAX_LINES,
            scroll: 0,
            follow: true,
            search_query: None,
            search_matches: Vec::new(),
            search_index: 0,
            log_path: None,
            last_position: 0,
        }
    }

    /// Create a new log viewer with custom max lines.
    pub fn with_max_lines(max_lines: usize) -> Self {
        Self {
            max_lines,
            ..Self::new()
        }
    }

    /// Clear the log buffer and reset state.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
        self.search_matches.clear();
        self.search_index = 0;
        self.last_position = 0;
    }

    pub fn set_log_path(&mut self, path: PathBuf) -> std::io::Result<()> {
        if self.log_path.as_ref() != Some(&path) {
            self.clear();
            self.log_path = Some(path);
            self.load_from_file()?;
        }

        Ok(())
    }

    /// Load or refresh log content from the file.
    pub fn load_from_file(&mut self) -> std::io::Result<()> {
        let path = match &self.log_path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        if !path.exists() {
            return Ok(());
        }

        let file = std::fs::File::open(&path)?;
        let mut reader = BufReader::new(file);

        if self.last_position > 0 {
            reader.seek(SeekFrom::Start(self.last_position))?;
        }

        let mut new_lines = Vec::new();
        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            new_lines.push(std::mem::take(&mut line));
        }

        // Update last position.
        self.last_position = reader.stream_position()?;

        // Add new lines to buffer.
        for new_line in new_lines {
            self.push_line(new_line);
        }

        // Update scroll if following.
        if self.follow && !self.lines.is_empty() {
            self.scroll = self.lines.len().saturating_sub(1);
        }

        // Update search matches if there's an active search.
        if self.search_query.is_some() {
            self.update_search_matches();
        }

        Ok(())
    }

    pub fn load_from_string(&mut self, content: &str) {
        self.clear();
        for line in content.lines() {
            self.push_line(line.to_string());
        }
        if self.follow && !self.lines.is_empty() {
            self.scroll = self.lines.len().saturating_sub(1);
        }
    }

    /// Get all log lines.
    pub fn lines(&self) -> VecDeque<&String> {
        self.lines.iter().collect()
    }

    /// Get the number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get the current scroll position.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Check if follow mode is enabled.
    pub fn follow(&self) -> bool {
        self.follow
    }

    /// Toggle follow mode.
    pub fn toggle_follow(&mut self) {
        self.follow = !self.follow;
        if self.follow && !self.is_empty() {
            self.scroll = self.lines.len().saturating_sub(1);
        }
    }

    /// Set follow mode.
    pub fn set_follow(&mut self, follow: bool) {
        self.follow = follow;
        if follow && !self.is_empty() {
            self.scroll = self.lines.len().saturating_sub(1);
        }
    }

    /// Scroll up by a number of lines.
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
        self.follow = false;
    }

    /// Scroll down by a number of lines.
    pub fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.lines.len().saturating_sub(1);
        self.scroll = (self.scroll + amount).min(max_scroll);
    }

    /// Scroll to the top.
    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
        self.follow = false;
    }

    /// Scroll to the bottom.
    pub fn scroll_to_bottom(&mut self) {
        if !self.lines.is_empty() {
            self.scroll = self.lines.len().saturating_sub(1);
        }
        self.follow = true;
    }

    /// Get visible lines for a given viewport height.
    pub fn visible_lines(&self, height: usize) -> Vec<&str> {
        let start = self.scroll;
        let end = (start + height).min(self.lines.len());
        self.lines.range(start..end).map(|s| s.as_str()).collect()
    }

    /// Set the search query and find matches.
    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_search();
            return;
        }

        self.search_query = Some(query.to_lowercase());
        self.update_search_matches();
        self.search_index = 0;

        // Jump to first match if any.
        if let Some(&first_match) = self.search_matches.first() {
            self.scroll = first_match;
            self.follow = false;
        }
    }

    /// Clear the search query and matches.
    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.search_matches.clear();
        self.search_index = 0;
    }

    pub fn search_query(&self) -> Option<&str> {
        self.search_query.as_deref()
    }

    /// Get the number of search matches.
    pub fn search_match_count(&self) -> usize {
        self.search_matches.len()
    }

    /// Get the current search match index (1-based for display).
    pub fn current_search_index(&self) -> usize {
        if self.search_matches.is_empty() {
            0
        } else {
            self.search_index + 1
        }
    }

    /// Jump to the next search match.
    pub fn next_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        self.search_index = (self.search_index + 1) % self.search_matches.len();
        self.scroll = self.search_matches[self.search_index];
        self.follow = false;
    }

    /// Jump to the previous search match.
    pub fn prev_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }

        self.search_index = if self.search_index == 0 {
            self.search_matches.len() - 1
        } else {
            self.search_index - 1
        };
        self.scroll = self.search_matches[self.search_index];
        self.follow = false;
    }

    /// Check if a line index is a search match.
    pub fn is_search_match(&self, line_index: usize) -> bool {
        self.search_matches.contains(&line_index)
    }

    /// Update search matches after content changes.
    fn update_search_matches(&mut self) {
        let quety = match &self.search_query {
            Some(p) => p.clone(),
            None => return,
        };

        self.search_matches.clear();
        for (i, line) in self.lines.iter().enumerate() {
            if line.to_lowercase().contains(&quety) {
                self.search_matches.push(i);
            }
        }
    }

    /// Push a new line to the buffer.
    fn push_line(&mut self, line: String) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            // Adjust scroll if we removed lines from the top.
            if self.scroll > 0 {
                self.scroll = self.scroll.saturating_sub(1);
            }
        }
        self.lines.push_back(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_viewer_load_string() {
        let mut viewer = LogViewer::new();
        viewer.load_from_string("line1\nline2\nline3");

        assert_eq!(viewer.len(), 3);
        assert_eq!(viewer.lines()[0], "line1");
        assert_eq!(viewer.lines()[1], "line2");
        assert_eq!(viewer.lines()[2], "line3");
    }

    #[test]
    fn test_log_viewer_scroll() {
        let mut viewer = LogViewer::new();
        viewer.load_from_string("line1\nline2\nline3\nline4\nline5");
        viewer.set_follow(false);
        viewer.scroll = 0;

        viewer.scroll_down(2);
        assert_eq!(viewer.scroll(), 2);

        viewer.scroll_up(1);
        assert_eq!(viewer.scroll(), 1);

        viewer.scroll_to_bottom();
        assert_eq!(viewer.scroll(), 4);
        assert!(viewer.follow());

        viewer.scroll_to_top();
        assert_eq!(viewer.scroll(), 0);
        assert!(!viewer.follow());
    }

    #[test]
    fn test_log_viewer_search() {
        let mut viewer = LogViewer::new();
        viewer.load_from_string("INFO: test\nERROR: failed\nINFO: success\nERROR: crash");

        viewer.search("error");
        assert_eq!(viewer.search_match_count(), 2);
        assert_eq!(viewer.current_search_index(), 1);
        assert_eq!(viewer.scroll(), 1); // First ERROR line.

        viewer.next_search_match();
        assert_eq!(viewer.current_search_index(), 2);
        assert_eq!(viewer.scroll(), 3); // Second ERROR line.

        viewer.next_search_match();
        assert_eq!(viewer.current_search_index(), 1); // Wraps around.

        viewer.clear_search();
        assert_eq!(viewer.search_match_count(), 0);
        assert!(viewer.search_query().is_none());
    }

    #[test]
    fn test_log_viewer_max_lines() {
        let mut viewer = LogViewer::with_max_lines(3);
        viewer.load_from_string("line1\nline2\nline3\nline4\nline5");

        // Should only keep the last 3 lines.
        assert_eq!(viewer.len(), 3);
        assert_eq!(viewer.lines()[0], "line3");
        assert_eq!(viewer.lines()[1], "line4");
        assert_eq!(viewer.lines()[2], "line5");
    }

    #[test]
    fn test_log_viewer_visible_lines() {
        let mut viewer = LogViewer::new();
        viewer.load_from_string("line1\nline2\nline3\nline4\nline5");
        viewer.set_follow(false);
        viewer.scroll = 1;

        let visible = viewer.visible_lines(3);
        assert_eq!(visible.len(), 3);
        assert_eq!(visible[0], "line2");
        assert_eq!(visible[1], "line3");
        assert_eq!(visible[2], "line4");
    }

    #[test]
    fn test_toggle_follow() {
        let mut viewer = LogViewer::new();
        viewer.load_from_string("line1\nline2\nline3");

        assert!(viewer.follow());
        viewer.toggle_follow();
        assert!(!viewer.follow());
        viewer.toggle_follow();
        assert!(viewer.follow());
        assert_eq!(viewer.scroll(), 2); // Should jump to end.
    }
}
