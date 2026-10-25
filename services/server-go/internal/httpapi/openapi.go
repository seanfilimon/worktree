package httpapi

import (
	_ "embed"
	"net/http"
	"strconv"
)

// openAPISpec is the OpenAPI 3.1 description of every REST route of this
// package (docs/ENGINEERING_PLAN.md §14.10). TestOpenAPICoverage keeps it in
// step with the router: a route missing from the document, or a documented
// path the router does not serve, fails the build.
//
//go:embed openapi.yaml
var openAPISpec []byte

// registerOpenAPIRoutes serves the embedded document without authentication.
func (a *api) registerOpenAPIRoutes() {
	a.handle("GET /api/v1/openapi.yaml", a.handleOpenAPI)
}

// handleOpenAPI answers GET /api/v1/openapi.yaml.
func (a *api) handleOpenAPI(w http.ResponseWriter, r *http.Request) error {
	h := w.Header()
	h.Set("Content-Type", "application/yaml; charset=utf-8")
	h.Set("Content-Length", strconv.Itoa(len(openAPISpec)))
	h.Set("Cache-Control", "public, max-age=300")
	h.Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(openAPISpec)
	return nil
}
