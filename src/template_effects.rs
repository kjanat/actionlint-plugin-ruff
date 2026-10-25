//! Versioned semantic adapter. Do not classify diagnostics using message text.
use serde::Serialize;

pub const CONTRACT: &str = "kjanat:actionlint/template-effects@0.1.0";
pub const REVIEWED_ENGINE_VERSION: &str = "0.17.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateEffect {
    None,
    LineLength,
    CompleteReferences,
    OpaqueName,
    OpaqueParentheses,
    StringContents,
    DocstringSummary,
    ExportedName,
    PercentConversion,
}

pub fn classify(code: &str) -> TemplateEffect {
    match code {
        "E501" => TemplateEffect::LineLength,
        "F401" | "F811" | "F841" | "F842" | "ARG001" | "ARG002" | "ARG003" | "ARG004"
        | "ARG005" | "B007" => TemplateEffect::CompleteReferences,
        "F821" | "B018" => TemplateEffect::OpaqueName,
        "UP034" => TemplateEffect::OpaqueParentheses,
        "F501" => TemplateEffect::PercentConversion,
        "F502" | "F503" | "F504" | "F505" | "F506" | "F507" | "F508" | "F521" | "F522" | "F523"
        | "F524" | "F525" | "F541" => TemplateEffect::StringContents,
        "D400" | "D415" => TemplateEffect::DocstringSummary,
        "F822" => TemplateEffect::ExportedName,
        _ => TemplateEffect::None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn upstream_version_requires_semantic_review() {
        assert_eq!(crate::ENGINE_VERSION, super::REVIEWED_ENGINE_VERSION);
    }
}
