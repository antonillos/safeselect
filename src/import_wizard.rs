//! Rebuild import drafts from confirmed answers when navigating backwards.
use crate::error::{Result, SafeselectError};

#[derive(Clone, PartialEq)]
pub(crate) enum Answer {
    Text(String),
    Confirm(bool),
    Selection(Vec<usize>),
}

#[derive(Default)]
pub struct Wizard {
    history: Vec<(String, Answer)>,
    cursor: usize,
    replay_until: usize,
    context: String,
    #[cfg(test)]
    script: Option<std::collections::VecDeque<(String, Result<Answer>)>>,
}

impl Wizard {
    #[cfg(test)]
    pub(crate) fn scripted(steps: Vec<(&str, Result<Answer>)>) -> Self {
        Self {
            script: Some(
                steps
                    .into_iter()
                    .map(|(key, answer)| (key.into(), answer))
                    .collect(),
            ),
            ..Default::default()
        }
    }
    #[cfg(test)]
    pub(crate) fn assert_finished(&self) {
        assert!(self.script.as_ref().unwrap().is_empty());
    }

    pub fn run<T>(&mut self, mut prepare: impl FnMut(&mut Self) -> Result<T>) -> Result<Option<T>> {
        self.replay_until = 0;
        println!("Esc: previous step (cancel at the first step); Ctrl+C: cancel");
        loop {
            self.cursor = 0;
            self.context.clear();
            match prepare(self) {
                Ok(value) => {
                    self.history.clear();
                    return Ok(Some(value));
                }
                Err(SafeselectError::ImportBack) if self.cursor > 0 => {
                    self.replay_until = self.cursor - 1;
                }
                Err(SafeselectError::ImportBack | SafeselectError::ImportCancelled) => {
                    self.history.clear();
                    println!("Import cancelled. No changes saved.");
                    return Ok(None);
                }
                Err(error) => {
                    self.history.clear();
                    return Err(error);
                }
            }
        }
    }

    pub fn context(&mut self, context: impl Into<String>) {
        self.context = context.into();
    }

    pub fn scoped<T>(
        &mut self,
        scope: &str,
        action: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = self.context.clone();
        self.context = format!("{previous}:{scope}");
        let result = action(self);
        self.context = previous;
        result
    }

    fn ask(
        &mut self,
        key: String,
        read: impl FnOnce(Option<&Answer>) -> Result<Answer>,
    ) -> Result<Answer> {
        let key = format!("{}:{key}", self.context);
        if self
            .history
            .get(self.cursor)
            .is_some_and(|(old, _)| *old != key)
        {
            self.history.truncate(self.cursor);
        }
        let previous = self.history.get(self.cursor).map(|(_, answer)| answer);
        let answer = if self.cursor < self.replay_until {
            previous
                .cloned()
                .ok_or_else(|| SafeselectError::Other("Import history changed".into()))?
        } else {
            self.read_answer(&key, read)?
        };
        if self.cursor == self.history.len() {
            self.history.push((key, answer.clone()));
        } else {
            self.history[self.cursor] = (key, answer.clone());
        }
        self.cursor += 1;
        Ok(answer)
    }

    fn read_answer(
        &mut self,
        key: &str,
        read: impl FnOnce(Option<&Answer>) -> Result<Answer>,
    ) -> Result<Answer> {
        #[cfg(test)]
        if let Some(script) = &mut self.script {
            let (expected, answer) = script.pop_front().expect("unexpected import prompt");
            assert!(key.contains(&expected), "expected {expected}, got {key}");
            return answer;
        }
        let _ = key;
        read(self.history.get(self.cursor).map(|(_, answer)| answer))
    }

    pub fn text(&mut self, prompt: &str, default: &str) -> Result<String> {
        let answer = self.ask(format!("text:{prompt}:{default}"), |old| {
            let default = match old {
                Some(Answer::Text(value)) => value,
                _ => default,
            };
            inquire::Text::new(prompt)
                .with_default(default)
                .with_help_message("Esc: previous step; Ctrl+C: cancel")
                .prompt()
                .map(Answer::Text)
                .map_err(prompt_error)
        })?;
        match answer {
            Answer::Text(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    pub fn password(&mut self) -> Result<String> {
        let answer = self.ask("password".into(), |_| {
            inquire::Password::new("Password (hidden):")
                .without_confirmation()
                .with_help_message("Esc: previous step; Ctrl+C: cancel")
                .prompt()
                .map(Answer::Text)
                .map_err(prompt_error)
        })?;
        match answer {
            Answer::Text(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    pub fn confirm(&mut self, prompt: &str, default: bool) -> Result<bool> {
        let answer = self.ask(format!("confirm:{prompt}"), |old| {
            let default = match old {
                Some(Answer::Confirm(value)) => *value,
                _ => default,
            };
            inquire::Confirm::new(prompt)
                .with_default(default)
                .with_help_message("Esc: previous step; Ctrl+C: cancel")
                .prompt()
                .map(Answer::Confirm)
                .map_err(prompt_error)
        })?;
        match answer {
            Answer::Confirm(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    pub fn select(&mut self, prompt: &str, options: Vec<String>, default: usize) -> Result<String> {
        let answer = self.ask(format!("select:{prompt}:{options:?}"), |old| {
            let cursor = match old {
                Some(Answer::Selection(values)) => values[0],
                _ => default,
            };
            inquire::Select::new(prompt, options.clone())
                .with_render_config(crate::compass_import::selection_render_config(
                    prompt,
                    options
                        .iter()
                        .map(|value| value.chars().count())
                        .max()
                        .unwrap_or(0),
                ))
                .with_starting_cursor(cursor)
                .prompt()
                .map(|value| {
                    Answer::Selection(vec![options
                        .iter()
                        .position(|item| *item == value)
                        .unwrap()])
                })
                .map_err(prompt_error)
        })?;
        match answer {
            Answer::Selection(values) => Ok(options[values[0]].clone()),
            _ => unreachable!(),
        }
    }

    pub fn multi(&mut self, prompt: &str, options: Vec<String>) -> Result<Vec<usize>> {
        let answer = self.ask(format!("multi:{prompt}:{options:?}"), |old| {
            let defaults = match old {
                Some(Answer::Selection(values)) => values.as_slice(),
                _ => &[],
            };
            // Labels are unique by index, even when the exported names collide.
            let labels: Vec<String> = options
                .iter()
                .enumerate()
                .map(|(i, label)| format!("{}. {label}", i + 1))
                .collect();
            let selected = inquire::MultiSelect::new(prompt, labels.clone())
                .with_render_config(crate::compass_import::selection_render_config(
                    prompt,
                    labels.iter().map(|value| value.chars().count()).sum(),
                ))
                .with_default(defaults)
                .with_page_size(20)
                .with_help_message("Esc: previous step; Ctrl+C: cancel")
                .prompt()
                .map_err(prompt_error)?;
            Ok(Answer::Selection(
                selected
                    .iter()
                    .map(|label| labels.iter().position(|item| item == label).unwrap())
                    .collect(),
            ))
        })?;
        match answer {
            Answer::Selection(values) => Ok(values),
            _ => unreachable!(),
        }
    }
}

fn prompt_error(error: inquire::InquireError) -> SafeselectError {
    match error {
        inquire::InquireError::OperationCanceled => SafeselectError::ImportBack,
        inquire::InquireError::OperationInterrupted => SafeselectError::ImportCancelled,
        _ => SafeselectError::Other("Import prompt failed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisiting_restores_confirmed_answers_and_skips_only_prior_questions() {
        let mut wizard = Wizard::default();
        let mut visits = 0;
        let value = wizard
            .run(|ui| {
                let first = ui.ask("first".into(), |_| Ok(Answer::Text("confirmed".into())))?;
                ui.ask("second".into(), |previous| {
                    visits += 1;
                    if visits == 1 {
                        assert!(previous.is_none());
                    } else {
                        assert!(
                            matches!(previous, Some(Answer::Text(value)) if value == "second value")
                        );
                    }
                    Ok(Answer::Text("second value".into()))
                })?;
                ui.ask("last".into(), |_| {
                    if visits == 1 {
                        Err(SafeselectError::ImportBack)
                    } else {
                        Ok(Answer::Confirm(true))
                    }
                })?;
                Ok(first)
            })
            .unwrap()
            .unwrap();
        assert!(matches!(value, Answer::Text(value) if value == "confirmed"));
        assert_eq!(visits, 2);
        assert!(wizard.history.is_empty());
    }

    #[test]
    fn changing_branch_invalidates_incompatible_answers() {
        let mut wizard = Wizard::default();
        let mut branch = 0;
        let mut password_visits = 0;
        wizard
            .run(|ui| {
                ui.ask("branch".into(), |_| {
                    branch += 1;
                    Ok(Answer::Confirm(branch > 1))
                })?;
                let key = if branch == 1 { "password" } else { "key file" };
                ui.ask(key.into(), |previous| {
                    if branch == 1 {
                        password_visits += 1;
                        if password_visits > 1 {
                            return Err(SafeselectError::ImportBack);
                        }
                    }
                    assert!(previous.is_none());
                    Ok(Answer::Text("value".into()))
                })?;
                ui.ask("done".into(), |_| {
                    if branch == 1 {
                        Err(SafeselectError::ImportBack)
                    } else {
                        Ok(Answer::Confirm(true))
                    }
                })?;
                Ok(())
            })
            .unwrap();
        assert_eq!(branch, 2);
    }

    #[test]
    fn prompt_errors_distinguish_navigation_interruption_and_failure() {
        assert!(matches!(
            prompt_error(inquire::InquireError::OperationCanceled),
            SafeselectError::ImportBack
        ));
        assert!(matches!(
            prompt_error(inquire::InquireError::OperationInterrupted),
            SafeselectError::ImportCancelled
        ));
        assert!(matches!(
            prompt_error(inquire::InquireError::NotTTY),
            SafeselectError::Other(_)
        ));
    }
}
