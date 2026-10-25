package httpapi

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"mime"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// maxJSONBody caps the size of JSON request bodies.
const maxJSONBody = 1 << 20 // 1 MiB

// Error codes of the JSON error envelope.
const (
	codeBadRequest      = "bad_request"
	codeUnauthenticated = "unauthenticated"
	codeForbidden       = "forbidden"
	codeNotFound        = "not_found"
	codeConflict        = "conflict"
	codeInternal        = "internal"
)

// apiError is an error with an explicit HTTP status and envelope code.
type apiError struct {
	status int
	code   string
	msg    string
}

func (e *apiError) Error() string { return e.msg }

func badRequestf(format string, args ...any) error {
	return &apiError{status: http.StatusBadRequest, code: codeBadRequest, msg: fmt.Sprintf(format, args...)}
}

func notFoundf(format string, args ...any) error {
	return &apiError{status: http.StatusNotFound, code: codeNotFound, msg: fmt.Sprintf(format, args...)}
}

func conflictf(format string, args ...any) error {
	return &apiError{status: http.StatusConflict, code: codeConflict, msg: fmt.Sprintf(format, args...)}
}

func forbiddenf(format string, args ...any) error {
	return &apiError{status: http.StatusForbidden, code: codeForbidden, msg: fmt.Sprintf(format, args...)}
}

var errLoginRequired = &apiError{status: http.StatusUnauthorized, code: codeUnauthenticated, msg: "authentication required"}

// errorBody is the JSON error envelope.
type errorBody struct {
	Error errorDetail `json:"error"`
}

type errorDetail struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

// classify maps err onto an HTTP status, envelope code and client-facing
// message. Unrecognised errors become 500 with a generic message so that
// internal details never leak to clients.
func classify(err error) (status int, code, msg string) {
	var ae *apiError
	var denied *domain.PushDenied
	switch {
	case errors.As(err, &ae):
		return ae.status, ae.code, ae.msg
	case errors.As(err, &denied):
		code := denied.Kind
		if code == "" {
			code = codeForbidden
		}
		msg := denied.Reason
		if msg == "" {
			msg = denied.Error()
		}
		return http.StatusForbidden, code, msg
	case errors.Is(err, domain.ErrInvalid):
		return http.StatusBadRequest, codeBadRequest, err.Error()
	case errors.Is(err, domain.ErrUnauthenticated):
		return http.StatusUnauthorized, codeUnauthenticated, err.Error()
	case errors.Is(err, domain.ErrForbidden):
		return http.StatusForbidden, codeForbidden, err.Error()
	case errors.Is(err, domain.ErrNotFound), errors.Is(err, objstore.ErrNotFound):
		return http.StatusNotFound, codeNotFound, err.Error()
	case errors.Is(err, domain.ErrAlreadyExists), errors.Is(err, domain.ErrConflict):
		return http.StatusConflict, codeConflict, err.Error()
	default:
		return http.StatusInternalServerError, codeInternal, "internal server error"
	}
}

// writeError renders err with the JSON error envelope.
func (a *api) writeError(w http.ResponseWriter, r *http.Request, err error) {
	status, code, msg := classify(err)
	if status >= 500 {
		a.log.ErrorContext(r.Context(), "request failed",
			slog.String("request_id", requestIDFrom(r.Context())),
			slog.String("method", r.Method),
			slog.String("path", r.URL.Path),
			slog.Any("error", err))
	}
	if status == http.StatusUnauthorized {
		w.Header().Set("WWW-Authenticate", `Bearer realm="worktree"`)
	}
	if status == http.StatusForbidden {
		a.auditDenied(r, code, msg)
	}
	writeJSON(w, status, errorBody{Error: errorDetail{Code: code, Message: msg}})
}

// auditDenied records access.denied for a 403 answered by the REST API
// (§13.3). The tenant is the {tenant} of the route when it resolves.
func (a *api) auditDenied(r *http.Request, code, msg string) {
	if a.d.Audit == nil {
		return
	}
	route := r.Pattern
	if route == "" {
		route = r.Method + " " + r.URL.Path
	}
	details := map[string]string{"method": r.Method, "path": r.URL.Path, "route": route, "code": code}
	if act := deniedAction(msg); act != "" {
		details["action"] = act
	}
	tenantID := ""
	if ref := r.PathValue("tenant"); ref != "" {
		if t, err := a.tenantByRef(r.Context(), ref); err == nil {
			tenantID = t.ID
		}
	}
	a.audit(r, "access.denied", tenantID, "route", route, details)
}

// deniedAction extracts an action name ("tree:write") from an
// authorization error message, when it mentions one.
func deniedAction(msg string) string {
	for _, f := range strings.FieldsFunc(msg, func(r rune) bool { return r == ' ' || r == '"' || r == '(' || r == ')' || r == ',' }) {
		if i := strings.IndexByte(f, ':'); i > 0 && i < len(f)-1 && !strings.ContainsAny(f, "/.") {
			return strings.TrimRight(f, ":;")
		}
	}
	return ""
}

// writeJSON writes v as a JSON response with the given status.
func writeJSON(w http.ResponseWriter, status int, v any) {
	buf, err := json.Marshal(v)
	if err != nil {
		buf, _ = json.Marshal(errorBody{Error: errorDetail{Code: codeInternal, Message: "failed to encode response"}})
		status = http.StatusInternalServerError
	}
	h := w.Header()
	h.Set("Content-Type", "application/json; charset=utf-8")
	h.Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(status)
	_, _ = w.Write(append(buf, '\n'))
}

// decodeJSON decodes the request body into dst. The body is limited to
// maxJSONBody bytes, unknown fields are rejected and exactly one JSON value
// is accepted.
func decodeJSON(w http.ResponseWriter, r *http.Request, dst any) error {
	if ct := r.Header.Get("Content-Type"); ct != "" {
		mt, _, err := mime.ParseMediaType(ct)
		if err != nil || (mt != "application/json" && !strings.HasSuffix(mt, "+json")) {
			return &apiError{status: http.StatusUnsupportedMediaType, code: "unsupported_media_type",
				msg: "request body must be application/json"}
		}
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxJSONBody)
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(dst); err != nil {
		return jsonDecodeError(err)
	}
	if err := dec.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		var mbe *http.MaxBytesError
		if errors.As(err, &mbe) {
			return jsonDecodeError(err)
		}
		return badRequestf("request body must contain a single JSON value")
	}
	return nil
}

func jsonDecodeError(err error) error {
	var (
		mbe *http.MaxBytesError
		se  *json.SyntaxError
		ute *json.UnmarshalTypeError
	)
	switch {
	case errors.As(err, &mbe):
		return &apiError{status: http.StatusRequestEntityTooLarge, code: "payload_too_large",
			msg: fmt.Sprintf("request body exceeds %d bytes", mbe.Limit)}
	case errors.Is(err, io.EOF):
		return badRequestf("request body is required")
	case errors.Is(err, io.ErrUnexpectedEOF):
		return badRequestf("request body is truncated JSON")
	case errors.As(err, &se):
		return badRequestf("malformed JSON at offset %d", se.Offset)
	case errors.As(err, &ute):
		if ute.Field != "" {
			return badRequestf("field %q must be of type %s", ute.Field, ute.Type)
		}
		return badRequestf("JSON value must be of type %s", ute.Type)
	case strings.HasPrefix(err.Error(), "json: unknown field "):
		return badRequestf("unknown field %s", strings.TrimPrefix(err.Error(), "json: unknown field "))
	default:
		return badRequestf("invalid JSON body: %v", err)
	}
}

// nonNil returns s, or an empty slice when s is nil, so that lists encode as
// [] rather than null.
func nonNil[T any](s []T) []T {
	if s == nil {
		return []T{}
	}
	return s
}
