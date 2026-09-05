const REVIEW_PREFIX = "The following is the Codex agent history";
const REVIEW_CONTINUATION = /added since your last approval|Continue the same review conversation/i;
const EMBEDDED_USER = /\[\d+]\s+user:\s*(.+)/;
const SKILL_PATH = /^\[\$([^\]]+)\]\(/;
const SKILL_LINK = /^\[\$([^\]]+)\]\(([^)]+)\)/;
const CODEX_REQUEST_REVIEW = "Codex request review";
const REVIEW_CONTINUATION_LABEL = "Review continuation";

export interface LeadingSkillLink {
  name: string;
  path: string;
  rest: string;
}

/** Leading `[$name](path)` written by Codex skill invocation. */
export function parseLeadingSkillLink(text: string): LeadingSkillLink | null {
  const trimmed = text.trimStart();
  const match = trimmed.match(SKILL_LINK);
  if (!match) return null;
  return {
    name: match[1],
    path: match[2],
    rest: trimmed.slice(match[0].length).replace(/^\s+/, ""),
  };
}

export interface ReviewPromptDisplay {
  kind: "initial" | "continuation";
  headline: string;
}

/** Leading Codex request-review wrapper, not the human prompt. */
export function parseReviewPrompt(text: string): ReviewPromptDisplay | null {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith(REVIEW_PREFIX)) return null;
  if (REVIEW_CONTINUATION.test(trimmed.slice(0, 400))) {
    return { kind: "continuation", headline: REVIEW_CONTINUATION_LABEL };
  }
  return {
    kind: "initial",
    headline: firstEmbeddedUserLine(trimmed) ?? CODEX_REQUEST_REVIEW,
  };
}

function firstEmbeddedUserLine(text: string): string | null {
  const start = text.indexOf(">>> TRANSCRIPT START");
  const body = start >= 0 ? text.slice(start) : text;
  const line = body.match(EMBEDDED_USER)?.[1]?.trim();
  return line ? sessionHeadlineLead(line) : null;
}

export interface ReviewAssessmentDisplay {
  outcome: string | null;
  riskLevel: string | null;
  authorization: string | null;
  rationale: string | null;
}

function stringField(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/** Whole-message Codex approval JSON with outcome or risk_level. */
export function parseReviewAssessment(text: string): ReviewAssessmentDisplay | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    const record = parsed as Record<string, unknown>;
    const outcome = stringField(record.outcome);
    const riskLevel = stringField(record.risk_level);
    if (!outcome && !riskLevel) return null;
    return {
      outcome,
      riskLevel,
      authorization: stringField(record.user_authorization),
      rationale: stringField(record.rationale),
    };
  } catch {
    return null;
  }
}

/** Reading copy: skill `$name` or the review headline, not machine wrappers. */
export function userMessageReadingText(text: string): string {
  const skill = parseLeadingSkillLink(text);
  if (skill) return skill.rest ? `$${skill.name}\n${skill.rest}` : `$${skill.name}`;
  const review = parseReviewPrompt(text);
  if (review) return review.headline;
  return text;
}

export interface SessionListCopy {
  headline: string;
  supporting: string | null;
  emptyPreview: boolean;
}

export function sessionSkillName(text: string): string | null {
  const match = text.trim().match(/^\[\$([^\]]+)\]/);
  return match?.[1] ?? null;
}

export function isHumanReadableSessionText(text: string): boolean {
  const value = text.trim();
  if (!value) return false;
  if (value.startsWith(REVIEW_PREFIX)) return false;
  if (SKILL_PATH.test(value)) return false;
  if (value.startsWith("{")) {
    try {
      const parsed: unknown = JSON.parse(value);
      return typeof parsed !== "object" || parsed === null || Array.isArray(parsed);
    } catch {
      return !/^\{\s*"/.test(value);
    }
  }
  return true;
}

/**
 * First paragraph or first CJK sentence of promoted latest copy. Collapsed
 * list titles also cut leftover markdown severity bullets.
 */
export function sessionHeadlineLead(text: string): string {
  const line =
    text
      .trim()
      .split(/\n\s*\n/, 1)[0]
      ?.split("\n", 1)[0]
      ?.trim() ?? "";
  const sentence = line.match(/^[\s\S]*?[。！？]/)?.[0]?.trim();
  const lead = sentence || line;
  const clipped = lead
    .replace(/\s+-\s+\*\*.*$/, "")
    .replace(/\s+\*\*(?:High|Medium|Low)\*\*\s+—.*$/i, "")
    .replace(/\s+(?:High|Medium|Low)\s+—.*$/i, "")
    .trim();
  return clipped || lead;
}

/** Catalog-only: drop common Markdown markers and keep the words. */
export function sessionCatalogPlainText(text: string): string {
  return text
    .replace(/\*\*/g, "")
    .replace(/`/g, "")
    .replace(/(^|\s)>\s+/g, "$1")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^[-*]\s+/, "");
}

/** Catalog primary line: human-readable copy, with native title or skill name demoted. */
export function sessionListCopy(
  title: string,
  latestMessage: string | null | undefined,
): SessionListCopy {
  const native = title.trim();
  const latest = latestMessage?.trim() ?? "";
  const skill = sessionSkillName(native);
  const review = native.startsWith(REVIEW_PREFIX) || latest.startsWith(REVIEW_PREFIX);
  const latestLead = isHumanReadableSessionText(latest) ? sessionHeadlineLead(latest) : latest;
  const headline = isHumanReadableSessionText(native)
    ? native
    : isHumanReadableSessionText(latest)
      ? latestLead
      : (skill ?? (review ? CODEX_REQUEST_REVIEW : native || "Untitled Session"));
  const demoted =
    skill && skill !== headline
      ? skill
      : native && native !== headline && isHumanReadableSessionText(native)
        ? native
        : null;
  const supportingRaw =
    demoted ?? (isHumanReadableSessionText(latest) && latestLead !== headline ? latestLead : null);
  const supporting = supportingRaw ? sessionCatalogPlainText(supportingRaw) || null : null;
  return {
    headline,
    supporting,
    emptyPreview:
      supporting === null &&
      headline !== CODEX_REQUEST_REVIEW &&
      !isHumanReadableSessionText(latest) &&
      !isHumanReadableSessionText(native),
  };
}
