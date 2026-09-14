pub fn split_args(s: &str) -> Vec<String> {
  let mut args = Vec::new();
  let mut chars = s.chars().peekable();
  let mut current = String::new();
  let mut in_quotes = false;

  while let Some(ch) = chars.next() {
    match ch {
      '"' => {
        // Toggle quote mode — quotes themselves are not copied
        in_quotes = !in_quotes;
      }
      ' ' | '\t' if !in_quotes => {
        if !current.is_empty() {
          args.push(current);
          current = String::new();
        }
        // Skip consecutive whitespace outside quotes
        while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
          chars.next();
        }
      }
      _ => current.push(ch),
    }
  }

  if !current.is_empty() {
    args.push(current);
  }

  args
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn simple_args() {
    assert_eq!(split_args("a b c"), vec!["a", "b", "c"]);
  }

  #[test]
  fn multiple_spaces() {
    assert_eq!(split_args("a   b\t\tc"), vec!["a", "b", "c"]);
  }

  #[test]
  fn quoted_path_with_spaces() {
    assert_eq!(
      split_args(r#"--path "C:\Program Files\Game" --flag"#),
      vec!["--path", r#"C:\Program Files\Game"#, "--flag"]
    );
  }

  #[test]
  fn empty_input() {
    assert_eq!(split_args(""), Vec::<String>::new());
  }

  #[test]
  fn only_spaces() {
    assert_eq!(split_args("   "), Vec::<String>::new());
  }

  #[test]
  fn trailing_quote() {
    // Unterminated quote — content still captured
    assert_eq!(split_args(r#"hello "world"#), vec!["hello", "world"]);
  }
}
