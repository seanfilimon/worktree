package notify

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Collections of other workstreams read here (§13.14).
const collMRComments = "mr_comments"

// EventReleaseUpdated is published by the release service when a release
// changes (e.g. a draft is published).
const EventReleaseUpdated = "release.updated"

// MaxBody bounds the excerpt stored in Notification.Body.
const MaxBody = 500

// mentionRe finds @name mentions: an @ not preceded by a word character,
// '.', '/', '-' or another '@' (so e-mail addresses do not match).
var mentionRe = regexp.MustCompile(`(?:^|[^A-Za-z0-9_.@/-])@([A-Za-z0-9][A-Za-z0-9-]{0,62})`)

// Mentions returns the distinct lower-cased names @mentioned in text, in
// order of appearance.
func Mentions(text string) []string {
	var out []string
	for _, m := range mentionRe.FindAllStringSubmatch(text, -1) {
		name := strings.ToLower(strings.TrimRight(m[1], "-"))
		if name != "" && !slices.Contains(out, name) {
			out = append(out, name)
		}
	}
	return out
}

// plan collects the recipients of one event.
type plan struct {
	tenant *domain.Tenant
	tree   *domain.Tree
	event  domain.Event

	title, body, url string
	// watch is the reason given to watchers ("" = not a watched event).
	watch string
	// exclude are usernames never notified (the actor, the requester).
	exclude map[string]bool

	reasons map[string]string // account id -> reason
	order   []string
}

func rank(reason string) int {
	if i := slices.Index(Reasons, reason); i >= 0 {
		return i
	}
	return len(Reasons)
}

// add gives accountID reason unless it already has a more specific one.
func (pl *plan) add(accountID, reason string) {
	if accountID == "" {
		return
	}
	cur, ok := pl.reasons[accountID]
	if !ok {
		pl.order = append(pl.order, accountID)
	}
	if !ok || rank(reason) < rank(cur) {
		pl.reasons[accountID] = reason
	}
}

// decode converts an event payload (a struct, pointer or map) into T.
func decode[T any](payload any) (T, bool) {
	var v T
	raw, err := json.Marshal(payload)
	if err != nil {
		return v, false
	}
	return v, json.Unmarshal(raw, &v) == nil
}

func excerpt(s string) string {
	s = strings.TrimSpace(s)
	if len(s) <= MaxBody {
		return s
	}
	cut := MaxBody
	for cut > 0 && !utf8.RuneStart(s[cut]) {
		cut--
	}
	return s[:cut] + "…"
}

func (pl *plan) treePath(section string) string {
	p := "/trees/" + pl.tenant.Slug + "/" + pl.tree.Name
	if section != "" {
		p += "/" + section
	}
	return p
}

func (pl *plan) mrPath(n int) string { return pl.treePath("merge-requests/" + strconv.Itoa(n)) }

func actorName(e domain.Event) string {
	if e.Actor == "" {
		return "someone"
	}
	return e.Actor
}

// mrRef is "!3 Title".
func mrRef(mr *domain.MergeRequest) string {
	if mr.Title == "" {
		return "!" + strconv.Itoa(mr.Number)
	}
	return fmt.Sprintf("!%d %s", mr.Number, mr.Title)
}

// HandleEvent creates the notifications of e and returns them (Start calls
// it for each bus event; tests call it directly).
func (s *Service) HandleEvent(ctx context.Context, e domain.Event) ([]domain.Notification, error) {
	if e.Type == domain.EventTreeDeleted {
		if e.TenantID != "" && e.TreeID != "" {
			return nil, s.deleteTree(ctx, e.TenantID, e.TreeID)
		}
		return nil, nil
	}
	if e.Type == domain.EventNotification || e.TenantID == "" || e.TreeID == "" || !handled(e.Type) {
		return nil, nil
	}
	tree, err := s.d.Store.GetTree(ctx, e.TreeID)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	tenant, err := s.d.Store.GetTenant(ctx, tree.TenantID)
	if err != nil {
		return nil, err
	}
	pl := &plan{tenant: tenant, tree: tree, event: e, exclude: map[string]bool{}, reasons: map[string]string{}}
	if e.Actor != "" {
		pl.exclude[e.Actor] = true
	}
	ok, err := s.build(ctx, pl)
	if err != nil || !ok {
		return nil, err
	}
	if pl.watch != "" {
		ws, err := s.watchers(ctx, tree)
		if err != nil {
			return nil, err
		}
		for _, id := range ws {
			pl.add(id, pl.watch)
		}
	}
	return s.fanOut(ctx, pl)
}

func handled(typ string) bool {
	switch typ {
	case domain.EventMROpened, domain.EventMRReviewRequested, domain.EventMRReady, domain.EventMRCommented,
		domain.EventMRReviewed, domain.EventMRApproved, domain.EventMRMerged, domain.EventMRClosed,
		domain.EventSuggestionApplied, domain.EventCIStatus, domain.EventTodoCreated, domain.EventTodoUpdated,
		domain.EventReleaseCreated, EventReleaseUpdated:
		return true
	}
	return false
}

// fanOut creates the planned notifications.
func (s *Service) fanOut(ctx context.Context, pl *plan) ([]domain.Notification, error) {
	var out []domain.Notification
	var errs []error
	for _, accountID := range pl.order {
		reason := pl.reasons[accountID]
		acct, err := s.d.Store.GetAccount(ctx, accountID)
		if errors.Is(err, domain.ErrNotFound) {
			continue
		}
		if err != nil {
			errs = append(errs, err)
			continue
		}
		if acct.Disabled || pl.exclude[acct.Username] {
			continue
		}
		watch := s.effectiveWatch(ctx, accountID, pl.tree)
		if watch.Level == domain.WatchIgnoring {
			continue
		}
		if reason == ReasonWatching || reason == ReasonRelease {
			if watch.Level != domain.WatchWatching {
				continue
			}
		}
		if !s.mayRead(ctx, acct, pl.tree) {
			continue
		}
		n := domain.Notification{
			AccountID: accountID,
			Reason:    reason,
			Event:     pl.event.Type,
			TenantID:  pl.tree.TenantID,
			TreeID:    pl.tree.ID,
			Title:     pl.title,
			Body:      pl.body,
			URL:       pl.url,
			Actor:     pl.event.Actor,
		}
		created, err := s.deliver(ctx, n, acct, watch)
		if err != nil {
			errs = append(errs, err)
			continue
		}
		if created != nil {
			out = append(out, *created)
		}
	}
	return out, errors.Join(errs...)
}

// mayRead reports whether acct holds tree:read on tree.
func (s *Service) mayRead(ctx context.Context, acct *domain.Account, tree *domain.Tree) bool {
	if s.d.Authz == nil {
		return true
	}
	p := &domain.Principal{AccountID: acct.ID, Username: acct.Username, TenantID: acct.TenantID, IsAdmin: acct.IsAdmin}
	return s.d.Authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree) == nil
}

// ---- recipients --------------------------------------------------------------

func (s *Service) accountByName(ctx context.Context, username string) string {
	a, err := s.d.Store.GetAccountByUsername(ctx, strings.ToLower(username))
	if err != nil {
		return ""
	}
	return a.ID
}

// teamMembers returns the account ids of team slug of tenantID (nil when
// there is no such team).
func (s *Service) teamMembers(ctx context.Context, tenantID, slug string) []string {
	t, err := s.d.Store.GetTeamBySlug(ctx, tenantID, strings.ToLower(slug))
	if err != nil {
		return nil
	}
	ms, err := s.d.Store.ListTeamMembers(ctx, t.ID)
	if err != nil {
		return nil
	}
	out := make([]string, 0, len(ms))
	for _, m := range ms {
		out = append(out, m.AccountID)
	}
	return out
}

// addReviewers adds "user:x" / "team:y" / "x" reviewers with reason.
func (s *Service) addReviewers(ctx context.Context, pl *plan, reviewers []string, reason string) {
	for _, r := range reviewers {
		kind, name, ok := strings.Cut(strings.TrimSpace(r), ":")
		if !ok {
			kind, name = "user", kind
		}
		switch kind {
		case "user":
			pl.add(s.accountByName(ctx, name), reason)
		case "team":
			for _, id := range s.teamMembers(ctx, pl.tree.TenantID, name) {
				pl.add(id, reason)
			}
		}
	}
}

// addMentions notifies the accounts (or members of the tree tenant's
// teams) @mentioned in text.
func (s *Service) addMentions(ctx context.Context, pl *plan, text string) {
	for _, name := range Mentions(text) {
		if id := s.accountByName(ctx, name); id != "" {
			pl.add(id, ReasonMentioned)
			continue
		}
		for _, id := range s.teamMembers(ctx, pl.tree.TenantID, name) {
			pl.add(id, ReasonMentioned)
		}
	}
}

func (s *Service) loadMR(ctx context.Context, treeID string, number int) *domain.MergeRequest {
	if number <= 0 {
		return nil
	}
	mr, err := s.d.Store.GetMergeRequest(ctx, treeID, number)
	if err != nil {
		return nil
	}
	return mr
}

// reviewersOf reads the v0.7 MergeRequest.Reviewers field from a payload
// (absent before the review-v07 workstream lands).
func reviewersOf(payload any) []string {
	v, _ := decode[struct {
		Reviewers []string `json:"reviewers"`
	}](payload)
	return v.Reviewers
}

// ---- per-event rules ---------------------------------------------------------

// build fills pl for its event; false = nothing to notify.
func (s *Service) build(ctx context.Context, pl *plan) (bool, error) {
	e := pl.event
	who := actorName(e)
	switch e.Type {
	case domain.EventMROpened:
		mr, ok := decode[domain.MergeRequest](e.Payload)
		if !ok || mr.Number == 0 {
			return false, nil
		}
		pl.title = fmt.Sprintf("%s opened %s", who, mrRef(&mr))
		pl.body, pl.url, pl.watch = excerpt(mr.Description), pl.mrPath(mr.Number), ReasonWatching
		s.addMentions(ctx, pl, mr.Title+"\n"+mr.Description)
		s.addReviewers(ctx, pl, reviewersOf(e.Payload), ReasonReviewRequested)

	case domain.EventMRReviewRequested:
		v, ok := decode[struct {
			Number      int      `json:"number"`
			Reviewers   []string `json:"reviewers"`
			RequestedBy string   `json:"requested_by"`
		}](e.Payload)
		if !ok || v.Number == 0 {
			return false, nil
		}
		mr := s.loadMR(ctx, pl.tree.ID, v.Number)
		ref := "!" + strconv.Itoa(v.Number)
		if mr != nil {
			ref = mrRef(mr)
		}
		if v.RequestedBy != "" {
			pl.exclude[v.RequestedBy] = true
			who = v.RequestedBy
		}
		pl.title = fmt.Sprintf("%s requested a review on %s", who, ref)
		pl.body = "Reviewers: " + strings.Join(v.Reviewers, ", ")
		pl.url, pl.watch = pl.mrPath(v.Number), ReasonWatching
		s.addReviewers(ctx, pl, v.Reviewers, ReasonReviewRequested)

	case domain.EventMRReady:
		mr, ok := decode[domain.MergeRequest](e.Payload)
		if !ok || mr.Number == 0 {
			return false, nil
		}
		pl.title = fmt.Sprintf("%s marked %s ready for review", who, mrRef(&mr))
		pl.url, pl.watch = pl.mrPath(mr.Number), ReasonWatching
		s.addReviewers(ctx, pl, reviewersOf(e.Payload), ReasonReviewRequested)

	case domain.EventMRCommented:
		c, ok := decode[domain.ReviewComment](e.Payload)
		if !ok || c.MRNumber == 0 {
			return false, nil
		}
		mr := s.loadMR(ctx, pl.tree.ID, c.MRNumber)
		ref := "!" + strconv.Itoa(c.MRNumber)
		if mr != nil {
			ref = mrRef(mr)
			pl.add(mr.AuthorID, ReasonMRCommented)
		}
		where := ""
		if c.Path != "" {
			where = " (" + c.Path
			if c.Line > 0 {
				where += ":" + strconv.Itoa(c.Line)
			}
			where += ")"
		}
		pl.title = fmt.Sprintf("%s commented on %s%s", who, ref, where)
		pl.body, pl.url, pl.watch = excerpt(c.Body), pl.mrPath(c.MRNumber), ReasonWatching
		s.addMentions(ctx, pl, c.Body)

	case domain.EventMRReviewed:
		v, ok := decode[struct {
			MR        domain.MergeRequest `json:"merge_request"`
			Decision  string              `json:"decision"`
			Reviewer  string              `json:"reviewer"`
			CommentID string              `json:"comment_id"`
		}](e.Payload)
		if !ok || v.MR.Number == 0 {
			return false, nil
		}
		verb := "reviewed"
		switch v.Decision {
		case domain.ReviewChangesRequested:
			verb = "requested changes on"
		case domain.ReviewCommented:
			verb = "commented on"
		}
		pl.title = fmt.Sprintf("%s %s %s", who, verb, mrRef(&v.MR))
		pl.url, pl.watch = pl.mrPath(v.MR.Number), ReasonWatching
		pl.add(v.MR.AuthorID, ReasonMRCommented)
		body := approvalBody(&v.MR, e.Actor)
		if v.CommentID != "" {
			key := domain.DocKey(pl.tree.ID, fmt.Sprintf("%08d", v.MR.Number), v.CommentID)
			if c, err := meta.GetDocT[domain.ReviewComment](ctx, s.d.Store, collMRComments, key); err == nil {
				body = c.Body
			}
		}
		pl.body = excerpt(body)
		s.addMentions(ctx, pl, body)

	case domain.EventMRApproved:
		mr, ok := decode[domain.MergeRequest](e.Payload)
		if !ok || mr.Number == 0 {
			return false, nil
		}
		pl.title = fmt.Sprintf("%s approved %s", who, mrRef(&mr))
		pl.url, pl.watch = pl.mrPath(mr.Number), ReasonWatching
		pl.add(mr.AuthorID, ReasonMRCommented)
		body := approvalBody(&mr, e.Actor)
		pl.body = excerpt(body)
		s.addMentions(ctx, pl, body)

	case domain.EventMRMerged:
		mr, ok := decode[domain.MergeRequest](e.Payload)
		if !ok || mr.Number == 0 {
			return false, nil
		}
		pl.title = fmt.Sprintf("%s merged %s into %s", who, mrRef(&mr), mr.TargetBranch)
		pl.url, pl.watch = pl.mrPath(mr.Number), ReasonWatching
		pl.add(mr.AuthorID, ReasonMRMerged)

	case domain.EventMRClosed:
		mr, ok := decode[domain.MergeRequest](e.Payload)
		if !ok || mr.Number == 0 {
			return false, nil
		}
		pl.title = fmt.Sprintf("%s closed %s", who, mrRef(&mr))
		pl.url, pl.watch = pl.mrPath(mr.Number), ReasonWatching

	case domain.EventSuggestionApplied:
		v, _ := decode[struct {
			Number int                  `json:"number"`
			MR     *domain.MergeRequest `json:"merge_request"`
		}](e.Payload)
		n := v.Number
		if n == 0 && v.MR != nil {
			n = v.MR.Number
		}
		mr := s.loadMR(ctx, pl.tree.ID, n)
		if mr == nil {
			return false, nil
		}
		pl.title = fmt.Sprintf("%s applied a suggestion to %s", who, mrRef(mr))
		pl.url, pl.watch = pl.mrPath(mr.Number), ReasonWatching
		pl.add(mr.AuthorID, ReasonMRCommented)

	case domain.EventCIStatus:
		return s.buildCI(ctx, pl)

	case domain.EventTodoCreated, domain.EventTodoUpdated:
		return s.buildTodo(ctx, pl)

	case domain.EventReleaseCreated, EventReleaseUpdated:
		return s.buildRelease(ctx, pl)

	default:
		return false, nil
	}
	return true, nil
}

// approvalBody is the review text actor left in mr's latest approval
// record.
func approvalBody(mr *domain.MergeRequest, actor string) string {
	for i := len(mr.Approvals) - 1; i >= 0; i-- {
		if mr.Approvals[i].Username == actor {
			return mr.Approvals[i].Body
		}
	}
	return ""
}

// buildCI: a failed or errored status notifies the author and pusher of
// the open merge requests whose source tip it is, and the watchers.
func (s *Service) buildCI(ctx context.Context, pl *plan) (bool, error) {
	st, ok := decode[domain.CIStatus](pl.event.Payload)
	if !ok || (st.State != domain.CIFailure && st.State != domain.CIError) {
		return false, nil
	}
	snap, err := s.d.Store.GetSnapshot(ctx, st.SnapshotID)
	if errors.Is(err, domain.ErrNotFound) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	short := st.SnapshotID
	if len(short) > 12 {
		short = short[:12]
	}
	pl.title = fmt.Sprintf("CI %s: %s on %s (%s)", st.State, st.Context, snap.BranchName, short)
	pl.body = excerpt(st.Description)
	pl.url = pl.treePath("snapshots/" + st.SnapshotID)
	pl.watch = ReasonWatching
	mrs, err := s.d.Store.ListMergeRequests(ctx, pl.tree.ID, domain.MRStateOpen)
	if err != nil {
		return false, err
	}
	authors := []string{snap.PushedBy}
	if id := s.accountByName(ctx, snap.Author); id != "" {
		authors = append(authors, id)
	}
	for i := range mrs {
		mr := &mrs[i]
		b, err := s.d.Store.GetBranch(ctx, pl.tree.ID, mr.SourceBranch)
		if err != nil || b.Tip != st.SnapshotID {
			continue
		}
		pl.title = fmt.Sprintf("CI %s: %s on %s", st.State, st.Context, mrRef(mr))
		pl.url = pl.mrPath(mr.Number)
		pl.add(mr.AuthorID, ReasonCIFailed)
		for _, id := range authors {
			pl.add(id, ReasonCIFailed)
		}
	}
	// The reporter (usually a CI service account) is the actor; a human
	// reporting their own failure is not notified about it.
	return true, nil
}

// buildTodo: a TODO whose assignee changed notifies the new assignee.
func (s *Service) buildTodo(ctx context.Context, pl *plan) (bool, error) {
	t, ok := decode[domain.Todo](pl.event.Payload)
	if !ok || t.Number == 0 {
		return false, nil
	}
	key := domain.DocKey("todo", pl.tree.ID, fmt.Sprintf("%010d", t.Number))
	changed := false
	err := meta.UpdateDocT(ctx, s.d.Store, CollState, key, func(cur *stateDoc) (*stateDoc, error) {
		prev := ""
		if cur != nil {
			prev = cur.Value
		}
		if prev == t.AssigneeID {
			return cur, nil
		}
		changed = true
		if t.AssigneeID == "" {
			return nil, nil
		}
		return &stateDoc{Value: t.AssigneeID, At: s.now()}, nil
	})
	if err != nil || !changed || t.AssigneeID == "" {
		return false, err
	}
	pl.title = fmt.Sprintf("TODO #%d assigned to you: %s", t.Number, t.Title)
	pl.body = excerpt(t.Details)
	pl.url = pl.treePath("todos")
	pl.add(t.AssigneeID, ReasonTodoAssigned)
	return true, nil
}

// stateDoc is a notify_state document.
type stateDoc struct {
	Value string    `json:"value"`
	At    time.Time `json:"at"`
}

// buildRelease: a release that is (or becomes) published notifies the
// watchers, once.
func (s *Service) buildRelease(ctx context.Context, pl *plan) (bool, error) {
	r, ok := decode[domain.Release](pl.event.Payload)
	if !ok || r.Tag == "" || r.Draft {
		return false, nil
	}
	key := domain.DocKey("release", pl.tree.ID, r.Tag)
	fresh := false
	err := meta.UpdateDocT(ctx, s.d.Store, CollState, key, func(cur *stateDoc) (*stateDoc, error) {
		if cur != nil && cur.Value == r.ID {
			return cur, nil
		}
		fresh = true
		return &stateDoc{Value: r.ID, At: s.now()}, nil
	})
	if err != nil || !fresh {
		return false, err
	}
	name := r.Name
	if name == "" {
		name = r.Tag
	}
	kind := "Release"
	if r.Prerelease {
		kind = "Pre-release"
	}
	pl.title = fmt.Sprintf("%s %s published in %s/%s", kind, name, pl.tenant.Slug, pl.tree.Name)
	pl.body = excerpt(r.Notes)
	pl.url = pl.treePath("releases")
	pl.watch = ReasonRelease
	return true, nil
}
