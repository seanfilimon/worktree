package httpapi

import (
	"net/http"
	"strconv"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/todo"
)

// registerTodoRoutes registers the v0.5 routes for TODOs of a tree and the
// dependency report (docs/ENGINEERING_PLAN.md §13.5). Reads need
// tree:read, writes tree:write. Without Deps.Todos they answer 501.
func (a *api) registerTodoRoutes(tree string) {
	a.handle("GET "+tree+"/todos", a.handleListTodos)
	a.handle("POST "+tree+"/todos", a.handleCreateTodo)
	a.handle("GET "+tree+"/todos/{n}", a.handleGetTodo)
	a.handle("PATCH "+tree+"/todos/{n}", a.handlePatchTodo)
	a.handle("POST "+tree+"/todos/{n}/claim", a.handleClaimTodo)
	a.handle("POST "+tree+"/todos/{n}/complete", a.handleCompleteTodo)
	a.handle("POST "+tree+"/todos/{n}/cancel", a.handleCancelTodo)
	a.handle("GET "+tree+"/dependencies", a.handleDependencies)
}

func (a *api) todos() (*todo.Service, error) {
	if a.d.Todos == nil {
		return nil, errIAMNotImplemented("TODOs")
	}
	return a.d.Todos, nil
}

// todoNumber parses the {n} path value.
func todoNumber(r *http.Request) (int, error) {
	n, err := strconv.Atoi(r.PathValue("n"))
	if err != nil || n < 1 {
		return 0, badRequestf("todo number must be a positive integer")
	}
	return n, nil
}

type todoList struct {
	Todos []domain.Todo `json:"todos"`
}

// handleListTodos answers GET {tree}/todos?status=&branch=&assignee=.
// status is a comma-separated list (also "active" = open,in_progress and
// "all"); assignee "me" is the caller.
func (a *api) handleListTodos(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.todos()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	statuses, err := todo.ParseStatuses(q.Get("status"))
	if err != nil {
		return err
	}
	f := todo.Filter{Statuses: statuses, Branch: q.Get("branch"), Assignee: q.Get("assignee")}
	if f.Assignee == "me" {
		p, err := requireLogin(r)
		if err != nil {
			return err
		}
		f.Assignee = p.Username
	}
	list, err := svc.List(r.Context(), tree.ID, f)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, todoList{Todos: nonNil(list)})
	return nil
}

// handleCreateTodo answers POST {tree}/todos (a manual TODO).
func (a *api) handleCreateTodo(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.todos()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeWrite)
	if err != nil {
		return err
	}
	var in todo.CreateInput
	if err := decodeJSON(w, r, &in); err != nil {
		return err
	}
	t, err := svc.Create(r.Context(), p, tree, in)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, t)
	return nil
}

// handleGetTodo answers GET {tree}/todos/{n}.
func (a *api) handleGetTodo(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.todos()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	n, err := todoNumber(r)
	if err != nil {
		return err
	}
	t, err := svc.Get(r.Context(), tree.ID, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, t)
	return nil
}

// todoWrite resolves the service, caller, tree and number of a TODO write.
func (a *api) todoWrite(r *http.Request) (*todo.Service, *domain.Principal, *domain.Tree, int, error) {
	svc, err := a.todos()
	if err != nil {
		return nil, nil, nil, 0, err
	}
	p, err := requireLogin(r)
	if err != nil {
		return nil, nil, nil, 0, err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeWrite)
	if err != nil {
		return nil, nil, nil, 0, err
	}
	n, err := todoNumber(r)
	if err != nil {
		return nil, nil, nil, 0, err
	}
	return svc, p, tree, n, nil
}

// handlePatchTodo answers PATCH {tree}/todos/{n}
// {title?, details?, priority?, blocking?, status?}.
func (a *api) handlePatchTodo(w http.ResponseWriter, r *http.Request) error {
	svc, p, tree, n, err := a.todoWrite(r)
	if err != nil {
		return err
	}
	var patch todo.Patch
	if err := decodeJSON(w, r, &patch); err != nil {
		return err
	}
	t, err := svc.Update(r.Context(), p, tree, n, patch)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, t)
	return nil
}

// handleClaimTodo answers POST {tree}/todos/{n}/claim.
func (a *api) handleClaimTodo(w http.ResponseWriter, r *http.Request) error {
	svc, p, tree, n, err := a.todoWrite(r)
	if err != nil {
		return err
	}
	t, err := svc.Claim(r.Context(), p, tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, t)
	return nil
}

type completeTodoRequest struct {
	Snapshot string `json:"snapshot"`
}

// handleCompleteTodo answers POST {tree}/todos/{n}/complete {snapshot?}.
// snapshot is a revision of the tree (id, branch, tag or id prefix).
func (a *api) handleCompleteTodo(w http.ResponseWriter, r *http.Request) error {
	svc, p, tree, n, err := a.todoWrite(r)
	if err != nil {
		return err
	}
	var req completeTodoRequest
	if err := decodeOptionalJSON(w, r, &req); err != nil {
		return err
	}
	snapID := ""
	if req.Snapshot != "" {
		s, err := a.snapshotForRev(r.Context(), tree, req.Snapshot)
		if err != nil {
			return err
		}
		snapID = s.ID
	}
	t, err := svc.Complete(r.Context(), p, tree, n, snapID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, t)
	return nil
}

// handleCancelTodo answers POST {tree}/todos/{n}/cancel.
func (a *api) handleCancelTodo(w http.ResponseWriter, r *http.Request) error {
	svc, p, tree, n, err := a.todoWrite(r)
	if err != nil {
		return err
	}
	t, err := svc.Cancel(r.Context(), p, tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, t)
	return nil
}

// handleDependencies answers GET {tree}/dependencies?branch=.
func (a *api) handleDependencies(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.todos()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	rep, err := svc.Dependencies(r.Context(), principal(r), tree, r.URL.Query().Get("branch"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, rep)
	return nil
}
