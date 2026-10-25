package httpapi

import (
	"bytes"
	"net/http"
	"reflect"
	"regexp"
	"slices"
	"sort"
	"strconv"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

// The OpenAPI tests keep openapi.yaml honest (docs/ENGINEERING_PLAN.md
// §14.10):
//
//   - TestOpenAPICoverage compares the patterns registered through
//     api.handle with the document's paths and methods, in both directions.
//     Adding a route means adding its operation to openapi.yaml (and the
//     reverse). Path wildcards {name...} are written {name} in the document.
//   - TestOpenAPIDocumentValid parses the document and checks every $ref,
//     operation ids, tags, x-wt-action and path parameters.
//   - TestOpenAPISchemasMatchGoTypes checks the component schemas listed in
//     openAPITypes (openapi_schema_test.go) against the Go JSON types.

// openAPIMethods are the operation keys of an OpenAPI path item.
var openAPIMethods = []string{"get", "put", "post", "delete", "options", "head", "patch", "trace"}

// loadOpenAPI parses the embedded document.
func loadOpenAPI(t *testing.T) map[string]any {
	t.Helper()
	var doc map[string]any
	if err := yaml.Unmarshal(openAPISpec, &doc); err != nil {
		t.Fatalf("openapi.yaml does not parse: %v", err)
	}
	return doc
}

// routerPatterns returns every pattern the router registers through handle.
func routerPatterns(t *testing.T) []string {
	t.Helper()
	f := newFixture(t)
	a := &api{d: f.deps, log: f.deps.Logger, mux: http.NewServeMux(), metrics: newMetrics()}
	a.stats = &statsCache{ttl: statsCacheTTL, compute: a.computeStats}
	a.routes()
	return a.patterns
}

var wildcardRe = regexp.MustCompile(`\{([A-Za-z0-9_]+)\.\.\.\}`)

// routeOperations normalises router patterns to "METHOD /path" with
// {name...} wildcards written {name}. The method-less "/api/" fallback
// (JSON 404/405) is not an operation.
func routeOperations(patterns []string) map[string]bool {
	ops := map[string]bool{}
	for _, p := range patterns {
		method, path, ok := strings.Cut(p, " ")
		if !ok {
			continue
		}
		ops[method+" "+wildcardRe.ReplaceAllString(path, "{$1}")] = true
	}
	return ops
}

// specOperations returns "METHOD /path" for every operation of doc.
func specOperations(t *testing.T, doc map[string]any) map[string]bool {
	t.Helper()
	paths, ok := doc["paths"].(map[string]any)
	if !ok {
		t.Fatal("openapi.yaml: paths is not a mapping")
	}
	ops := map[string]bool{}
	for path, item := range paths {
		m, ok := item.(map[string]any)
		if !ok {
			t.Fatalf("openapi.yaml: path item %s is not a mapping", path)
		}
		for _, method := range openAPIMethods {
			if _, ok := m[method]; ok {
				ops[strings.ToUpper(method)+" "+path] = true
			}
		}
	}
	return ops
}

func sortedOps(m map[string]bool) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}

func TestOpenAPICoverage(t *testing.T) {
	doc := loadOpenAPI(t)
	routes := routeOperations(routerPatterns(t))
	spec := specOperations(t, doc)
	if len(routes) == 0 {
		t.Fatal("no routes registered")
	}
	var missing, extra []string
	for _, op := range sortedOps(routes) {
		if !spec[op] {
			missing = append(missing, op)
		}
	}
	for _, op := range sortedOps(spec) {
		if !routes[op] {
			extra = append(extra, op)
		}
	}
	if len(missing) > 0 {
		t.Errorf("%d registered routes are missing from openapi.yaml:\n  %s", len(missing), strings.Join(missing, "\n  "))
	}
	if len(extra) > 0 {
		t.Errorf("openapi.yaml documents %d operations the router does not serve:\n  %s", len(extra), strings.Join(extra, "\n  "))
	}
}

func TestOpenAPIRouteOperationsNormalisation(t *testing.T) {
	got := routeOperations([]string{"GET /a/{x}/b/{rest...}", "/api/", "DELETE /c"})
	want := map[string]bool{"GET /a/{x}/b/{rest}": true, "DELETE /c": true}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("routeOperations = %v, want %v", got, want)
	}
}

// resolvePointer resolves a local JSON pointer ("#/components/schemas/X").
func resolvePointer(doc map[string]any, ref string) (any, bool) {
	if !strings.HasPrefix(ref, "#/") {
		return nil, false
	}
	var cur any = doc
	for _, part := range strings.Split(ref[2:], "/") {
		part = strings.ReplaceAll(strings.ReplaceAll(part, "~1", "/"), "~0", "~")
		m, ok := cur.(map[string]any)
		if !ok {
			return nil, false
		}
		if cur, ok = m[part]; !ok {
			return nil, false
		}
	}
	return cur, true
}

// walkRefs calls fn with the location and value of every "$ref" in v.
func walkRefs(v any, at string, fn func(at, ref string)) {
	switch x := v.(type) {
	case map[string]any:
		for k, child := range x {
			if k == "$ref" {
				s, _ := child.(string)
				fn(at, s)
				continue
			}
			walkRefs(child, at+"/"+k, fn)
		}
	case []any:
		for i, child := range x {
			walkRefs(child, at+"/"+strconv.Itoa(i), fn)
		}
	}
}

// deref follows $ref chains (parameters and responses may be references).
func deref(doc map[string]any, v any) map[string]any {
	for i := 0; i < 10; i++ {
		m, ok := v.(map[string]any)
		if !ok {
			return nil
		}
		ref, ok := m["$ref"].(string)
		if !ok {
			return m
		}
		if v, ok = resolvePointer(doc, ref); !ok {
			return nil
		}
	}
	return nil
}

var templateParamRe = regexp.MustCompile(`\{([^{}]+)\}`)

func TestOpenAPIDocumentValid(t *testing.T) {
	doc := loadOpenAPI(t)
	if v, _ := doc["openapi"].(string); !strings.HasPrefix(v, "3.1.") {
		t.Errorf("openapi = %q, want 3.1.x", v)
	}
	info, _ := doc["info"].(map[string]any)
	if info["title"] == nil || info["version"] == nil {
		t.Error("info.title and info.version are required")
	}

	// Every $ref resolves.
	refs := 0
	walkRefs(doc, "#", func(at, ref string) {
		refs++
		if _, ok := resolvePointer(doc, ref); !ok {
			t.Errorf("%s: $ref %q does not resolve", at, ref)
		}
	})
	if refs == 0 {
		t.Error("no $ref found; the document looks truncated")
	}

	comps, _ := doc["components"].(map[string]any)
	schemes, _ := comps["securitySchemes"].(map[string]any)
	if _, ok := schemes["bearerAuth"]; !ok {
		t.Error("components.securitySchemes.bearerAuth is missing")
	}
	checkSecurity := func(where string, v any) {
		reqs, ok := v.([]any)
		if !ok {
			t.Errorf("%s: security must be a list", where)
			return
		}
		for _, r := range reqs {
			m, _ := r.(map[string]any)
			for name := range m {
				if _, ok := schemes[name]; !ok {
					t.Errorf("%s: unknown security scheme %q", where, name)
				}
			}
		}
	}
	checkSecurity("security", doc["security"])

	tags := map[string]bool{}
	for _, tg := range doc["tags"].([]any) {
		tags[tg.(map[string]any)["name"].(string)] = true
	}

	opIDs := map[string]string{}
	paths := doc["paths"].(map[string]any)
	for _, path := range sortedKeysAny(paths) {
		item := paths[path].(map[string]any)
		if !strings.HasPrefix(path, "/") {
			t.Errorf("path %q must start with /", path)
		}
		if strings.Contains(path, "...") {
			t.Errorf("path %q: write wildcards as {name}, not {name...}", path)
		}
		templ := map[string]bool{}
		for _, m := range templateParamRe.FindAllStringSubmatch(path, -1) {
			templ[m[1]] = true
		}
		pathLevel := paramNames(t, doc, path, item["parameters"])
		for _, method := range openAPIMethods {
			raw, ok := item[method]
			if !ok {
				continue
			}
			where := strings.ToUpper(method) + " " + path
			op, ok := raw.(map[string]any)
			if !ok {
				t.Errorf("%s: operation is not a mapping", where)
				continue
			}
			id, _ := op["operationId"].(string)
			switch {
			case id == "":
				t.Errorf("%s: operationId is required", where)
			case opIDs[id] != "":
				t.Errorf("%s: operationId %q already used by %s", where, id, opIDs[id])
			default:
				opIDs[id] = where
			}
			if s, _ := op["summary"].(string); s == "" {
				t.Errorf("%s: summary is required", where)
			}
			opTags, _ := op["tags"].([]any)
			if len(opTags) == 0 {
				t.Errorf("%s: at least one tag is required", where)
			}
			for _, tg := range opTags {
				if s, _ := tg.(string); !tags[s] {
					t.Errorf("%s: tag %v is not declared in the top-level tags", where, tg)
				}
			}
			switch act := op["x-wt-action"].(type) {
			case string:
				if act == "" {
					t.Errorf("%s: x-wt-action is empty", where)
				}
			case []any:
				if len(act) == 0 {
					t.Errorf("%s: x-wt-action is an empty list", where)
				}
				for _, a := range act {
					if s, _ := a.(string); s == "" {
						t.Errorf("%s: x-wt-action entries must be non-empty strings", where)
					}
				}
			default:
				t.Errorf("%s: x-wt-action (string or list of strings) is required", where)
			}
			if sec, ok := op["security"]; ok {
				checkSecurity(where+" security", sec)
			}
			responses, _ := op["responses"].(map[string]any)
			if len(responses) == 0 {
				t.Errorf("%s: responses are required", where)
			}
			success := false
			for code := range responses {
				if len(code) == 3 && (code[0] == '2' || code[0] == '3') {
					success = true
				}
				if deref(doc, responses[code]) == nil {
					t.Errorf("%s: response %s is not an object", where, code)
				}
			}
			if !success {
				t.Errorf("%s: no 2xx/3xx response documented", where)
			}
			declared := map[string]bool{}
			for k, v := range pathLevel {
				declared[k] = v
			}
			for k, v := range paramNames(t, doc, where, op["parameters"]) {
				declared[k] = v
			}
			for name := range templ {
				if !declared[name] {
					t.Errorf("%s: path parameter {%s} is not declared", where, name)
				}
			}
			for name, inPath := range declared {
				if inPath && !templ[name] {
					t.Errorf("%s: path parameter %q does not appear in the path", where, name)
				}
			}
		}
	}
}

// paramNames returns name -> in=="path" for a parameters list.
func paramNames(t *testing.T, doc map[string]any, where string, v any) map[string]bool {
	t.Helper()
	out := map[string]bool{}
	list, _ := v.([]any)
	for _, p := range list {
		m := deref(doc, p)
		if m == nil {
			t.Errorf("%s: parameter %v does not resolve", where, p)
			continue
		}
		name, _ := m["name"].(string)
		in, _ := m["in"].(string)
		if name == "" || in == "" {
			t.Errorf("%s: parameter needs name and in: %v", where, m)
			continue
		}
		if in == "path" {
			if req, _ := m["required"].(bool); !req {
				t.Errorf("%s: path parameter %q must be required", where, name)
			}
		}
		out[name] = in == "path"
	}
	return out
}

func sortedKeysAny(m map[string]any) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}

func TestOpenAPISchemasMatchGoTypes(t *testing.T) {
	doc := loadOpenAPI(t)
	schemas, _ := doc["components"].(map[string]any)["schemas"].(map[string]any)
	for _, name := range sortedKeysAny(openAPITypes) {
		raw, ok := schemas[name]
		if !ok {
			t.Errorf("components.schemas.%s is missing (Go type %T)", name, openAPITypes[name])
			continue
		}
		s := raw.(map[string]any)
		props, _ := s["properties"].(map[string]any)
		fields := jsonFields(reflect.TypeOf(openAPITypes[name]))
		var want, wantReq []string
		for _, f := range fields {
			want = append(want, f.name)
			if !f.omitempty {
				wantReq = append(wantReq, f.name)
			}
		}
		got := sortedKeysAny(props)
		sort.Strings(want)
		if !slices.Equal(got, want) {
			t.Errorf("components.schemas.%s properties = %v, Go type %T encodes %v", name, got, openAPITypes[name], want)
		}
		var gotReq []string
		for _, r := range asList(s["required"]) {
			gotReq = append(gotReq, r.(string))
		}
		sort.Strings(gotReq)
		sort.Strings(wantReq)
		if isRequestSchema(name) {
			if len(gotReq) > 0 {
				t.Errorf("components.schemas.%s is a request body: list no required properties (got %v)", name, gotReq)
			}
			continue
		}
		if !slices.Equal(gotReq, wantReq) {
			t.Errorf("components.schemas.%s required = %v, want the non-omitempty fields %v", name, gotReq, wantReq)
		}
	}
}

func asList(v any) []any {
	l, _ := v.([]any)
	return l
}

func TestOpenAPIEndpoint(t *testing.T) {
	f := newFixture(t)
	rec := f.do(t, http.MethodGet, "/api/v1/openapi.yaml", "", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("GET /api/v1/openapi.yaml = %d: %s", rec.Code, rec.Body.String())
	}
	if ct := rec.Header().Get("Content-Type"); !strings.HasPrefix(ct, "application/yaml") {
		t.Errorf("Content-Type = %q", ct)
	}
	if !bytes.Equal(rec.Body.Bytes(), openAPISpec) {
		t.Error("body differs from the embedded document")
	}
	// Signed-in callers get the same document.
	if rec := f.do(t, http.MethodGet, "/api/v1/openapi.yaml", "alice", nil); rec.Code != http.StatusOK {
		t.Errorf("signed in: %d", rec.Code)
	}
	if rec := f.do(t, http.MethodPost, "/api/v1/openapi.yaml", "", nil); rec.Code != http.StatusMethodNotAllowed {
		t.Errorf("POST = %d, want 405", rec.Code)
	}
}
