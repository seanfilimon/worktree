package domain

import (
	"context"
	"time"
)

// ---- v0.4 IAM (docs/ENGINEERING_PLAN.md §12) ----

// Principal kinds.
const (
	PrincipalUser           = "user"
	PrincipalServiceAccount = "service_account"
)

// Token kinds.
const (
	TokenSession = "session" // issued by password / MFA / SSO login
	TokenAPI     = "api"     // personal API token
	TokenService = "service" // service-account key
)

// Additional actions (§12.3). Policies may also use "<area>:*" and "*".
const (
	ActBranchPush Action = "branch:push" // push to a specific branch (policy-level; implies tree:write)
	ActIAMRead    Action = "iam:read"    // view teams, roles, policies, grants, service accounts
	ActIAMAdmin   Action = "iam:admin"   // manage them
)

// Team team-member roles.
const (
	TeamRoleMaintainer = "maintainer" // may manage the team's membership
	TeamRoleMember     = "member"
)

// Grant subject types.
const (
	SubjectUser           = "user"
	SubjectTeam           = "team"
	SubjectServiceAccount = "service_account"
)

// Policy statement effects.
const (
	EffectAllow = "allow"
	EffectDeny  = "deny"
)

// Team is a named group of accounts inside a tenant.
type Team struct {
	ID          string    `json:"id"`
	TenantID    string    `json:"tenant_id"`
	Slug        string    `json:"slug"` // unique within tenant, [a-z0-9-]
	Name        string    `json:"name"`
	Description string    `json:"description"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

type TeamMember struct {
	TeamID    string    `json:"team_id"`
	AccountID string    `json:"account_id"`
	Role      string    `json:"role"` // TeamRoleMaintainer | TeamRoleMember
	CreatedAt time.Time `json:"created_at"`
}

// CustomRole is a tenant-defined role. Name must not collide with a
// built-in role. Members are assigned custom roles exactly like built-ins
// (Membership.Role, Grant.Role, TeamGrant, ServiceAccount.Role).
type CustomRole struct {
	TenantID    string    `json:"tenant_id"`
	Name        string    `json:"name"` // [a-z0-9-], 2..40
	Description string    `json:"description"`
	Permissions []Action  `json:"permissions"` // concrete actions or "<area>:*"
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

// PolicyConditions restrict when a statement applies. All set fields must
// hold (logical AND); an empty struct always holds.
type PolicyConditions struct {
	IPRanges   []string    `json:"ip_ranges,omitempty"`   // CIDRs; request IP must be in one
	RequireMFA bool        `json:"require_mfa,omitempty"` // principal's session used MFA
	NotBefore  *time.Time  `json:"not_before,omitempty"`
	NotAfter   *time.Time  `json:"not_after,omitempty"`
	TimeWindow *TimeWindow `json:"time_window,omitempty"`
}

// TimeWindow is a recurring weekly window, e.g. office hours.
type TimeWindow struct {
	Days     []string `json:"days"`     // "mon".."sun"; empty = every day
	Start    string   `json:"start"`    // "09:00"
	End      string   `json:"end"`      // "18:00" (exclusive; may wrap midnight)
	Timezone string   `json:"timezone"` // IANA name, default "UTC"
}

// PolicyStatement grants or denies actions on resources to principals.
//
// Resources: "wt:tenant/<slug>", "wt:tree/<tenant>/<tree>",
// "wt:branch/<tenant>/<tree>/<branch>", "wt:path/<tenant>/<tree>/<path>"
// with "*" (one segment) and "**" (any depth) wildcards, or "*" for all.
// Principals: "user:<username>", "team:<slug>", "role:<role>",
// "sa:<service-account>", "authenticated", "anonymous", "*".
// Actions: concrete Action strings, "<area>:*" or "*".
type PolicyStatement struct {
	Sid        string           `json:"sid,omitempty"`
	Effect     string           `json:"effect"` // EffectAllow | EffectDeny
	Actions    []string         `json:"actions"`
	Resources  []string         `json:"resources"`
	Principals []string         `json:"principals"`
	Conditions PolicyConditions `json:"conditions"`
}

// Policy is a named, versioned set of statements scoped to one tenant.
// Evaluation: an explicit deny in any enabled policy overrides every allow
// (roles, grants and policy allows). Policy allows extend role grants.
type Policy struct {
	ID          string            `json:"id"`
	TenantID    string            `json:"tenant_id"`
	Name        string            `json:"name"`
	Description string            `json:"description"`
	Enabled     bool              `json:"enabled"`
	Statements  []PolicyStatement `json:"statements"`
	Version     int               `json:"version"` // incremented on every update
	CreatedBy   string            `json:"created_by"`
	CreatedAt   time.Time         `json:"created_at"`
	UpdatedAt   time.Time         `json:"updated_at"`
}

// Grant assigns a role on one tree to a user, team or service account. A
// grant can raise a subject's access to a tree above its tenant role (or
// give access to a tree in a tenant it is not a member of).
type Grant struct {
	TreeID      string    `json:"tree_id"`
	SubjectType string    `json:"subject_type"` // SubjectUser | SubjectTeam | SubjectServiceAccount
	SubjectID   string    `json:"subject_id"`   // account / team / service-account id
	Subject     string    `json:"subject"`      // username / team slug / service-account name (display)
	Role        string    `json:"role"`         // built-in or custom role of the tree's tenant
	CreatedBy   string    `json:"created_by"`
	CreatedAt   time.Time `json:"created_at"`
}

// ServiceAccount is a non-human identity owned by a tenant. It
// authenticates with keys (APIToken with Kind TokenService) and acts with
// Role inside its tenant (plus tree grants).
type ServiceAccount struct {
	ID          string    `json:"id"`
	TenantID    string    `json:"tenant_id"`
	Name        string    `json:"name"` // [a-z0-9-], unique within tenant
	Description string    `json:"description"`
	Role        string    `json:"role"`
	Disabled    bool      `json:"disabled"`
	CreatedBy   string    `json:"created_by"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

// MFAState is an account's second-factor configuration.
type MFAState struct {
	AccountID          string     `json:"account_id"`
	TOTPEnabled        bool       `json:"totp_enabled"`
	TOTPSecret         string     `json:"-"` // base32; set once confirmed
	PendingSecret      string     `json:"-"` // base32; enrollment awaiting confirmation
	RecoveryCodeHashes []string   `json:"-"` // sha256 hex of unused codes
	LastUsedStep       int64      `json:"-"` // replay protection
	EnabledAt          *time.Time `json:"enabled_at,omitempty"`
	RecoveryCodesLeft  int        `json:"recovery_codes_left"`
}

// Invitation invites a person (by email or existing username) to a tenant.
type Invitation struct {
	ID         string     `json:"id"`
	TenantID   string     `json:"tenant_id"`
	Email      string     `json:"email,omitempty"`
	Username   string     `json:"username,omitempty"`
	Role       string     `json:"role"`
	TeamID     string     `json:"team_id,omitempty"` // also join this team
	CodeHash   string     `json:"-"`                 // sha256 hex of the one-time code
	InvitedBy  string     `json:"invited_by"`        // username
	CreatedAt  time.Time  `json:"created_at"`
	ExpiresAt  time.Time  `json:"expires_at"`
	AcceptedAt *time.Time `json:"accepted_at,omitempty"`
	AcceptedBy string     `json:"accepted_by,omitempty"`
	Revoked    bool       `json:"revoked"`
}

// PasswordReset is a one-time code that sets a new password (issued by a
// server admin; there is no email delivery).
type PasswordReset struct {
	CodeHash  string     `json:"-"`
	AccountID string     `json:"account_id"`
	CreatedBy string     `json:"created_by"`
	CreatedAt time.Time  `json:"created_at"`
	ExpiresAt time.Time  `json:"expires_at"`
	UsedAt    *time.Time `json:"used_at,omitempty"`
}

// SSOConfig configures OpenID Connect single sign-on (one provider).
type SSOConfig struct {
	Enabled        bool      `json:"enabled"`
	Name           string    `json:"name"` // button label, e.g. "Okta"
	Issuer         string    `json:"issuer"`
	ClientID       string    `json:"client_id"`
	ClientSecret   string    `json:"-"`
	HasSecret      bool      `json:"has_client_secret"`
	RedirectURL    string    `json:"redirect_url"` // https://host/api/v1/auth/sso/callback
	Scopes         []string  `json:"scopes"`       // default openid email profile
	AllowedDomains []string  `json:"allowed_domains"`
	AutoCreate     bool      `json:"auto_create"`    // create accounts on first login
	UsernameClaim  string    `json:"username_claim"` // default preferred_username
	UpdatedAt      time.Time `json:"updated_at"`
}

// SSOIdentity links an external (issuer, subject) to an account.
type SSOIdentity struct {
	Issuer      string    `json:"issuer"`
	Subject     string    `json:"subject"`
	AccountID   string    `json:"account_id"`
	Email       string    `json:"email"`
	CreatedAt   time.Time `json:"created_at"`
	LastLoginAt time.Time `json:"last_login_at"`
}

// IAMStore persists IAM entities. Embedded in MetaStore; implemented by
// internal/meta (Bolt + Memory). Same error/ordering conventions as
// MetaStore: wrapped ErrNotFound / ErrAlreadyExists, non-nil empty slices,
// copies returned. Deleting a team removes its members and grants;
// deleting a service account removes its keys and grants.
type IAMStore interface {
	// Teams (ordered by slug)
	CreateTeam(ctx context.Context, t *Team) error // assigns ID if empty
	GetTeam(ctx context.Context, id string) (*Team, error)
	GetTeamBySlug(ctx context.Context, tenantID, slug string) (*Team, error)
	ListTeams(ctx context.Context, tenantID string) ([]Team, error)
	UpdateTeam(ctx context.Context, t *Team) error
	DeleteTeam(ctx context.Context, id string) error
	PutTeamMember(ctx context.Context, m *TeamMember) error
	DeleteTeamMember(ctx context.Context, teamID, accountID string) error
	ListTeamMembers(ctx context.Context, teamID string) ([]TeamMember, error)
	ListAccountTeams(ctx context.Context, accountID string) ([]TeamMember, error)

	// Custom roles (ordered by name)
	PutRole(ctx context.Context, r *CustomRole) error
	GetRole(ctx context.Context, tenantID, name string) (*CustomRole, error)
	ListRoles(ctx context.Context, tenantID string) ([]CustomRole, error)
	DeleteRole(ctx context.Context, tenantID, name string) error

	// Policies (ordered by name)
	CreatePolicy(ctx context.Context, p *Policy) error // assigns ID if empty
	GetPolicy(ctx context.Context, tenantID, id string) (*Policy, error)
	ListPolicies(ctx context.Context, tenantID string) ([]Policy, error)
	UpdatePolicy(ctx context.Context, p *Policy) error
	DeletePolicy(ctx context.Context, tenantID, id string) error

	// Tree grants (keyed by tree+subject_type+subject_id; Put replaces)
	PutGrant(ctx context.Context, g *Grant) error
	ListGrants(ctx context.Context, treeID string) ([]Grant, error)
	ListSubjectGrants(ctx context.Context, subjectType, subjectID string) ([]Grant, error)
	DeleteGrant(ctx context.Context, treeID, subjectType, subjectID string) error

	// Service accounts (ordered by name)
	CreateServiceAccount(ctx context.Context, sa *ServiceAccount) error // assigns ID
	GetServiceAccount(ctx context.Context, id string) (*ServiceAccount, error)
	GetServiceAccountByName(ctx context.Context, tenantID, name string) (*ServiceAccount, error)
	ListServiceAccounts(ctx context.Context, tenantID string) ([]ServiceAccount, error)
	UpdateServiceAccount(ctx context.Context, sa *ServiceAccount) error
	DeleteServiceAccount(ctx context.Context, id string) error
	ListServiceAccountTokens(ctx context.Context, serviceAccountID string) ([]APIToken, error)

	// MFA (one state per account)
	GetMFA(ctx context.Context, accountID string) (*MFAState, error)
	PutMFA(ctx context.Context, m *MFAState) error
	DeleteMFA(ctx context.Context, accountID string) error

	// Invitations (ordered newest first)
	CreateInvitation(ctx context.Context, inv *Invitation) error // assigns ID
	GetInvitation(ctx context.Context, tenantID, id string) (*Invitation, error)
	GetInvitationByCodeHash(ctx context.Context, codeHash string) (*Invitation, error)
	ListInvitations(ctx context.Context, tenantID string) ([]Invitation, error)
	UpdateInvitation(ctx context.Context, inv *Invitation) error
	DeleteInvitation(ctx context.Context, tenantID, id string) error

	// Password resets
	PutPasswordReset(ctx context.Context, r *PasswordReset) error
	GetPasswordReset(ctx context.Context, codeHash string) (*PasswordReset, error)
	DeletePasswordReset(ctx context.Context, codeHash string) error

	// SSO
	GetSSOConfig(ctx context.Context) (*SSOConfig, error) // ErrNotFound if never set
	PutSSOConfig(ctx context.Context, c *SSOConfig) error
	PutSSOIdentity(ctx context.Context, id *SSOIdentity) error
	GetSSOIdentity(ctx context.Context, issuer, subject string) (*SSOIdentity, error)
	ListAccountSSOIdentities(ctx context.Context, accountID string) ([]SSOIdentity, error)
}

// Resource identifies what an action targets, for fine-grained evaluation.
type Resource struct {
	TenantID string
	Tree     *Tree  // nil for tenant-level actions
	Branch   string // optional
	Path     string // optional, '/'-separated within the tree
}

// RequestEnv carries request context for policy conditions.
type RequestEnv struct {
	RemoteIP string    // "" if unknown
	Now      time.Time // zero = time.Now()
}

// DecisionReason explains one input to a Decision.
type DecisionReason struct {
	// "server_admin", "disabled", "tenant_role", "team_role", "tree_grant",
	// "custom_role", "service_account", "visibility", "token_scope",
	// "policy", "default".
	Source string `json:"source"`
	Detail string `json:"detail"`
	Effect string `json:"effect"` // "allow" | "deny" | "info"
}

// Decision is the result of evaluating an access request.
type Decision struct {
	Allowed bool             `json:"allowed"`
	Effect  string           `json:"effect"` // "allow" | "deny" (explicit) | "implicit_deny"
	Reasons []DecisionReason `json:"reasons"`
}

// AccessEvaluator is the full authorization engine (implemented by
// internal/authz, which also implements Authorizer on top of it).
type AccessEvaluator interface {
	Evaluate(ctx context.Context, p *Principal, act Action, res Resource, env RequestEnv) (Decision, error)
	// EffectivePermissions lists every action p may perform on tree (or the
	// tenant when tree is nil), ignoring conditions that need a request.
	EffectivePermissions(ctx context.Context, p *Principal, tenantID string, tree *Tree) ([]Action, error)
}

// SessionMeta describes the client creating a session.
type SessionMeta struct {
	RemoteIP  string
	UserAgent string
}

// LoginResult is returned by the first login step.
type LoginResult struct {
	Token       string   // set when no second factor is required
	Account     *Account // always set on success
	MFARequired bool
	MFAToken    string // short-lived (5 min) token for CompleteMFALogin
}

// AccountSecurity covers sessions, MFA, scoped tokens, service-account keys,
// invitations and password resets. Implemented by internal/iam.Service.
type AccountSecurity interface {
	LoginWithMeta(ctx context.Context, username, password string, meta SessionMeta) (*LoginResult, error)
	CompleteMFALogin(ctx context.Context, mfaToken, code string, meta SessionMeta) (token string, acct *Account, err error)
	// IssueSession creates a session token directly (used by SSO).
	IssueSession(ctx context.Context, accountID string, mfa bool, meta SessionMeta) (string, error)

	MFAStatus(ctx context.Context, accountID string) (*MFAState, error) // zero state if none
	BeginTOTPEnroll(ctx context.Context, accountID string) (secret, otpauthURL string, err error)
	ConfirmTOTP(ctx context.Context, accountID, code string) (recoveryCodes []string, err error)
	DisableTOTP(ctx context.Context, accountID, password string) error
	RegenerateRecoveryCodes(ctx context.Context, accountID, code string) ([]string, error)

	ListSessions(ctx context.Context, accountID string) ([]APIToken, error)
	RevokeOtherSessions(ctx context.Context, accountID, keepTokenHash string) (int, error)

	CreateScopedToken(ctx context.Context, accountID, name string, ttl time.Duration, scopes []string) (string, *APIToken, error)
	CreateServiceAccountKey(ctx context.Context, serviceAccountID, name string, ttl time.Duration, scopes []string) (string, *APIToken, error)
	RevokeServiceAccountKey(ctx context.Context, serviceAccountID, tokenID string) error

	CreateInvitation(ctx context.Context, inv *Invitation, ttl time.Duration) (code string, err error)
	PreviewInvitation(ctx context.Context, code string) (*Invitation, *Tenant, error)
	AcceptInvitation(ctx context.Context, code string, p *Principal) (*Invitation, error)

	CreatePasswordReset(ctx context.Context, accountID, createdBy string, ttl time.Duration) (code string, expiresAt time.Time, err error)
	ResetPassword(ctx context.Context, code, newPassword string) error
}

// SSOProvider runs the OpenID Connect login flow. Implemented by internal/sso.
type SSOProvider interface {
	Config(ctx context.Context) (*SSOConfig, error) // zero config if unset
	SaveConfig(ctx context.Context, c *SSOConfig, clientSecret *string) (*SSOConfig, error)
	// AuthURL starts a login; redirectAfter is a same-origin path.
	AuthURL(ctx context.Context, redirectAfter string) (string, error)
	Callback(ctx context.Context, code, state string, meta SessionMeta) (token string, acct *Account, redirectAfter string, err error)
}
