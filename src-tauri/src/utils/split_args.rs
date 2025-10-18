pub fn split_args(s: &str) -> Vec<String> {
  let mut args = Vec::new();
  let mut chars = s.chars().peekable();
  let mut current = String::new();

  while let Some(ch) = chars.next() {
    match ch {
      ' ' | '\t' => {
        if !current.is_empty() {
          args.push(current);
          current = String::new();
        }
        // Пропускаем последующие пробелы
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
