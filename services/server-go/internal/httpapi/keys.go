package httpapi

import (
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/signing"
)

// registerKeyRoutes registers the v0.5 routes for signing keys of the
// caller and of accounts (docs/ENGINEERING_PLAN.md §13.6).
func (a *api) registerKeyRoutes() {
	a.handle("GET /api/v1/auth/keys", a.handleListKeys)
	a.handle("POST /api/v1/auth/keys", a.handleCreateKey)
	a.handle("DELETE /api/v1/auth/keys/{id}", a.handleRevokeKey)
	a.handle("GET /api/v1/accounts/{username}/keys", a.handleAccountKeys)
}

// signingSvc returns Deps.Signing or a 501 error.
func (a *api) signingSvc() (*signing.Service, error) {
	if a.d.Signing == nil {
		return nil, errIAMNotImplemented("snapshot signing")
	}
	return a.d.Signing, nil
}

// createSigningKeyRequest is the body of POST /api/v1/auth/keys. PublicKey is
// standard base64 in JSON.
type createSigningKeyRequest struct {
	Name      string `json:"name"`
	Algorithm string `json:"algorithm,omitempty"`
	PublicKey []byte `json:"public_key"`
}

// handleListKeys answers GET /api/v1/auth/keys: the caller's keys.
func (a *api) handleListKeys(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, err := a.signingSvc()
	if err != nil {
		return err
	}
	keys, err := svc.List(r.Context(), p.AccountID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, keys)
	return nil
}

// handleCreateKey answers POST /api/v1/auth/keys.
func (a *api) handleCreateKey(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, err := a.signingSvc()
	if err != nil {
		return err
	}
	var req createSigningKeyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	k, err := svc.Register(r.Context(), p, req.Name, req.Algorithm, req.PublicKey)
	if err != nil {
		return err
	}
	a.audit(r, "signing_key.create", p.TenantID, "signing_key", k.ID, map[string]string{
		"name": k.Name, "fingerprint": k.Fingerprint,
	})
	writeJSON(w, http.StatusCreated, k)
	return nil
}

// handleRevokeKey answers DELETE /api/v1/auth/keys/{id}: revokes one of
// the caller's keys (signatures by it no longer verify on push).
func (a *api) handleRevokeKey(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, err := a.signingSvc()
	if err != nil {
		return err
	}
	id := r.PathValue("id")
	k, err := svc.Revoke(r.Context(), p.AccountID, id)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("signing key %q not found", id)
		}
		return err
	}
	a.audit(r, "signing_key.revoke", p.TenantID, "signing_key", k.ID, map[string]string{
		"name": k.Name, "fingerprint": k.Fingerprint,
	})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleAccountKeys answers GET /api/v1/accounts/{username}/keys: the
// public keys of any account, for any authenticated caller.
func (a *api) handleAccountKeys(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, err := a.signingSvc()
	if err != nil {
		return err
	}
	username := r.PathValue("username")
	acct, err := a.d.Store.GetAccountByUsername(r.Context(), username)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("account %q not found", username)
		}
		return err
	}
	keys, err := svc.List(r.Context(), acct.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, keys)
	return nil
}
