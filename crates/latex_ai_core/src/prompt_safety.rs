use thiserror::Error;

#[derive(Debug, Error)]
pub enum PromptInjectionError {
    #[error("Instruction is empty or only whitespace")]
    Empty,
    #[error("Instruction exceeds maximum length")]
    TooLong,
    #[error("Blocked injection attempt")]
    Injection,
}

const PROMPT_INJECTION_PATTERNS: &[&str] = &[
    r"ignore\s+previous", r"system\s*:", r"<\|im_start\|>", r"###\s*Instruction",
    r"<\|\s*system\s*\|>",
];

pub fn sanitize(instruction: &str, max_len: usize) -> Result<String, PromptInjectionError> {
    if instruction.is_empty() || !instruction.chars().any(|c| !c.is_whitespace()) {
        return Err(PromptInjectionError::Empty);
    }
    if instruction.len() > max_len { return Err(PromptInjectionError::TooLong); }
    let cleaned: String = instruction.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r')).collect();
    let lowered = cleaned.to_lowercase();
    for pat in PROMPT_INJECTION_PATTERNS {
        if let Ok(re) = regex::Regex::new(pat) {
            if re.is_match(&lowered) {
                tracing::warn!(intent = %instruction.chars().take(100).collect::<String>(), "blocked injection attempt");
                return Err(PromptInjectionError::Injection);
            }
        }
    }
    Ok(cleaned.trim().to_string())
}
