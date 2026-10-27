// Convenience aliases over the generated OpenAPI types, plus precise shapes for
// fields the server documents only as `Object`.
import type { components } from './schema'

type S = components['schemas']

export type Me = S['Me']
export type UserPreferences = S['UserPreferences']
export type SiteInfo = S['SiteInfo']
export type SetSummary = S['SetSummary']
export type SetDetail = S['SetDetail']
export type SetProblemItem = S['SetProblemItem']
export type ProblemView = S['ProblemView']
export type Segment = S['Segment']
export type Draft = S['Draft']
export type Progress = S['Progress']
export type SystemStatus = S['SystemStatus']
export type TestView = S['TestView']
export type DiffLine = S['DiffLine']
export type SubmissionSummary = S['SubmissionSummary']
export type ProblemContent = S['ProblemContent']
export type ProblemDetail = S['ProblemDetail']
export type ProblemSummary = S['ProblemSummary']
export type RevisionSummary = S['RevisionSummary']
export type RevisionDetail = S['RevisionDetail']
export type ValidationReport = S['ValidationReport']
export type AdminSet = S['AdminSet']
export type ImportReport = S['ImportReport']
export type FieldError = S['FieldError']
export type CatalogOverview = S['CatalogOverview']
export type RuleListItem = S['RuleListItem']
export type TextsEditor = S['TextsEditor']
export type HistoryEntry = S['HistoryEntry']
export type AdminUser = S['AdminUser']
export type CreatedUser = S['CreatedUser']
export type BatchResult = S['BatchResult']
export type Cohort = S['Cohort']
export type PolicyPatch = S['PolicyPatch']
export type EffectivePolicy = S['EffectivePolicy']
export type PolicyView = S['PolicyView']
export type ProgressMatrix = S['ProgressMatrix']
export type UserProblemProgress = S['UserProblemProgress']
export type Round = S['Round']
export type Leaderboard = S['Leaderboard']
export type BoardRow = S['BoardRow']
export type Badge = S['Badge']
export type ActiveSet = S['ActiveSet']
export type RuleStat = S['RuleStat']
export type AdminSubmission = S['AdminSubmission']
export type Overview = S['Overview']
export type SettingsView = S['SettingsView']
export type SiteSettings = S['SiteSettings']
export type TokenInfo = S['TokenInfo']
export type CreatedToken = S['CreatedToken']
export type McpStatus = S['McpStatus']
export type AuthRequestView = S['AuthRequestView']
export type AuthDecisionResult = S['AuthDecisionResult']
export type AuditEntry = S['AuditEntry']
export type Concept = S['Concept']
export type ProblemLimits = S['ProblemLimits']
export type TestCaseInput = S['TestCaseInput']
export type Page<T> = { items: T[]; next_cursor?: string | null }

export interface Span {
  line: number
  column: number
  end_line: number
  end_column: number
}

export interface RawDiagnostic {
  origin: 'compiler' | 'linker' | 'lint' | 'sandbox'
  kind: string
  message: string
  option?: string | null
  line?: number | null
  column?: number | null
}

export interface TestRef {
  number: number
  hidden: boolean
  name?: string | null
}

/** A feedback card. Students get `CardView`; the admin preview returns the
 * full `Card` (all hints). Both normalise to this shape. */
export interface FeedbackCard {
  finding_id: number
  rule: string
  category: string
  severity: 'error' | 'warning' | 'info' | string
  confidence: 'confirmed' | 'likely' | 'possible' | string
  confidence_label: string
  title: string
  what: string
  cause?: string | null
  where_text?: string | null
  why?: string | null
  next_steps: string[]
  hints: string[]
  hints_total: number
  hints_allowed: number
  span?: Span | null
  precision: string
  precision_note?: string | null
  in_fixed_region: boolean
  fixed_region_note?: string | null
  primary: boolean
  maybe_cascade_of?: number | null
  cascade_note?: string | null
  test?: TestRef | null
  raw: RawDiagnostic[]
}

export type PlaygroundRun = S['PlaygroundRun']
export type PlaygroundResult = Omit<S['PlaygroundResult'], 'cards'> & { cards: FeedbackCard[] }

export type SubmissionView = Omit<S['SubmissionView'], 'cards' | 'blanks'> & {
  cards: FeedbackCard[]
  blanks?: Record<string, string> | null
}

export interface RuleVariant {
  when_concepts: string[]
  title?: string | null
  what?: string | null
  cause?: string | null
  where?: string | null
  why?: string | null
  next_steps?: string[] | null
  hints?: string[] | null
}

export interface RuleEntry {
  enabled: boolean
  priority: number
  title: string
  what: string
  cause: string
  where: string
  why: string
  next_steps: string[]
  hints: string[]
  variants: RuleVariant[]
}

export interface RuleEditorView {
  id: string
  vars: string[]
  builtin?: RuleEntry | null
  published: RuleEntry
  draft?: RuleEntry | null
  reset_pending: boolean
}

export interface CustomRule {
  id: string
  kind: 'error' | 'warning' | 'any'
  option?: string | null
  message_regex: string
  enabled: boolean
  priority: number
  title: string
  what: string
  cause: string
  where: string
  why: string
  next_steps: string[]
  hints: string[]
}

/** Full card from the admin preview (all hints, no reveal state). */
export interface PreviewCard extends Omit<FeedbackCard, 'hints_total' | 'hints_allowed'> {
  used_fallback_text: boolean
}

export interface PreviewResult {
  compile_success: boolean
  cards: PreviewCard[]
}

export function previewToFeedback(c: PreviewCard): FeedbackCard {
  return { ...c, hints_total: c.hints.length, hints_allowed: c.hints.length }
}
