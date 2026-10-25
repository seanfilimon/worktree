// Command chartcheck renders the W0rkTree Helm chart without Helm and checks
// the output. It is the fallback for `helm lint` / `helm template` on
// machines without Helm (CI runs both).
//
//	cd deploy/helm/check && go run . [-chart ../worktree] [-cases cases] [-v]
//
// For every values file in -cases (plus the chart defaults alone) it
//
//   - deep-merges the file over values.yaml (like `helm template -f`),
//   - validates the merged values against values.schema.json (the subset of
//     JSON Schema the chart uses),
//   - renders every template with Go text/template, missingkey=error and the
//     Sprig/Helm functions the chart uses (implemented below),
//   - parses every rendered document as YAML and checks the Kubernetes
//     basics (apiVersion, kind, metadata.name, DNS-1123 names, unique
//     kind/name pairs, base64 Secret data, container images and probes),
//   - compares the result with the case's "# expect:" / "# expect-error:"
//     header comments.
//
// Exit status is 0 when every case passes, 1 otherwise.
package main

import (
	"bufio"
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"math/big"
	"os"
	"path"
	"path/filepath"
	"reflect"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"text/template"

	"gopkg.in/yaml.v3"
)

func main() {
	chartDir := flag.String("chart", "../worktree", "chart directory")
	casesDir := flag.String("cases", "cases", "directory of values files to render (one case each)")
	verbose := flag.Bool("v", false, "print rendered manifests")
	flag.Parse()

	chart, err := loadChart(*chartDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "chartcheck: %v\n", err)
		os.Exit(1)
	}
	cases, err := loadCases(*casesDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "chartcheck: %v\n", err)
		os.Exit(1)
	}

	failed := 0
	for _, c := range cases {
		out, err := runCase(chart, c)
		status := "ok"
		if err != nil {
			status = "FAIL: " + err.Error()
			failed++
		}
		fmt.Printf("%-28s %s\n", c.name, status)
		if *verbose && out != "" {
			fmt.Println(out)
		}
	}
	if failed > 0 {
		fmt.Printf("%d of %d cases failed\n", failed, len(cases))
		os.Exit(1)
	}
	fmt.Printf("all %d cases passed\n", len(cases))
}

// ---------------------------------------------------------------- chart

type chartFiles struct {
	dir       string
	meta      map[string]any // Chart.yaml
	values    map[string]any // values.yaml
	schema    map[string]any // values.schema.json (may be nil)
	templates map[string]string
}

func loadChart(dir string) (*chartFiles, error) {
	c := &chartFiles{dir: dir, templates: map[string]string{}}
	if err := readYAML(filepath.Join(dir, "Chart.yaml"), &c.meta); err != nil {
		return nil, err
	}
	for _, k := range []string{"apiVersion", "name", "version", "appVersion"} {
		if s, _ := c.meta[k].(string); s == "" {
			return nil, fmt.Errorf("Chart.yaml: %s must be a non-empty string", k)
		}
	}
	if c.meta["apiVersion"] != "v2" {
		return nil, fmt.Errorf("Chart.yaml: apiVersion must be v2")
	}
	if !semverRe.MatchString(c.meta["version"].(string)) {
		return nil, fmt.Errorf("Chart.yaml: version %q is not SemVer 2", c.meta["version"])
	}
	if err := readYAML(filepath.Join(dir, "values.yaml"), &c.values); err != nil {
		return nil, err
	}
	if b, err := os.ReadFile(filepath.Join(dir, "values.schema.json")); err == nil {
		if err := json.Unmarshal(b, &c.schema); err != nil {
			return nil, fmt.Errorf("values.schema.json: %v", err)
		}
	}
	tdir := filepath.Join(dir, "templates")
	err := filepath.WalkDir(tdir, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		rel, _ := filepath.Rel(tdir, p)
		b, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		c.templates[path.Join(c.name(), "templates", filepath.ToSlash(rel))] = string(b)
		return nil
	})
	if err != nil {
		return nil, err
	}
	if len(c.templates) == 0 {
		return nil, errors.New("no templates found")
	}
	return c, nil
}

func (c *chartFiles) name() string { return c.meta["name"].(string) }

var semverRe = regexp.MustCompile(`^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$`)

func readYAML(p string, dst *map[string]any) error {
	b, err := os.ReadFile(p)
	if err != nil {
		return err
	}
	if err := yaml.Unmarshal(b, dst); err != nil {
		return fmt.Errorf("%s: %v", p, err)
	}
	if *dst == nil {
		*dst = map[string]any{}
	}
	*dst = normalize(*dst).(map[string]any)
	return nil
}

// normalize converts YAML-decoded values to the JSON-like shapes Helm
// templates see: map[string]any, []any, float64 numbers.
func normalize(v any) any {
	switch t := v.(type) {
	case map[string]any:
		for k, e := range t {
			t[k] = normalize(e)
		}
		return t
	case map[any]any:
		m := map[string]any{}
		for k, e := range t {
			m[fmt.Sprint(k)] = normalize(e)
		}
		return m
	case []any:
		for i, e := range t {
			t[i] = normalize(e)
		}
		return t
	case int:
		return float64(t)
	case int64:
		return float64(t)
	case uint64:
		return float64(t)
	default:
		return v
	}
}

// ---------------------------------------------------------------- cases

type testCase struct {
	name        string
	values      map[string]any
	expect      []string // "Kind/name" that must be rendered
	expectNot   []string // "Kind/name" that must not be rendered
	expectError string   // substring of the expected render/validation error
	expectText  []string // substrings that must appear in the rendered output
}

func loadCases(dir string) ([]testCase, error) {
	cases := []testCase{{name: "(chart defaults)"}}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".yaml") {
			continue
		}
		p := filepath.Join(dir, e.Name())
		tc := testCase{name: e.Name()}
		if err := readYAML(p, &tc.values); err != nil {
			return nil, err
		}
		f, err := os.Open(p)
		if err != nil {
			return nil, err
		}
		sc := bufio.NewScanner(f)
		for sc.Scan() {
			line := strings.TrimSpace(sc.Text())
			for prefix, dst := range map[string]*[]string{
				"# expect:":      &tc.expect,
				"# expect-not:":  &tc.expectNot,
				"# expect-text:": &tc.expectText,
			} {
				if strings.HasPrefix(line, prefix) {
					val := strings.TrimSpace(strings.TrimPrefix(line, prefix))
					if prefix == "# expect-text:" {
						*dst = append(*dst, val)
						continue
					}
					for _, s := range strings.Split(val, ",") {
						if s = strings.TrimSpace(s); s != "" {
							*dst = append(*dst, s)
						}
					}
				}
			}
			if strings.HasPrefix(line, "# expect-error:") {
				tc.expectError = strings.TrimSpace(strings.TrimPrefix(line, "# expect-error:"))
			}
		}
		f.Close()
		cases = append(cases, tc)
	}
	return cases, nil
}

func runCase(c *chartFiles, tc testCase) (string, error) {
	values := deepCopy(c.values).(map[string]any)
	if tc.values != nil {
		values = mergeValues(values, deepCopy(tc.values).(map[string]any))
	}
	var out string
	err := func() error {
		if c.schema != nil {
			if err := validateSchema(c.schema, values, "values"); err != nil {
				return fmt.Errorf("schema: %w", err)
			}
		}
		manifests, notes, err := render(c, values)
		if err != nil {
			return err
		}
		out = manifests
		if strings.TrimSpace(notes) == "" {
			return errors.New("NOTES.txt rendered empty")
		}
		objs, err := checkManifests(manifests)
		if err != nil {
			return err
		}
		for _, want := range tc.expect {
			if !objs[want] {
				return fmt.Errorf("expected %s to be rendered (got %s)", want, keys(objs))
			}
		}
		for _, unwanted := range tc.expectNot {
			if objs[unwanted] {
				return fmt.Errorf("expected %s not to be rendered", unwanted)
			}
		}
		for _, s := range tc.expectText {
			if !strings.Contains(manifests, s) {
				return fmt.Errorf("expected rendered output to contain %q", s)
			}
		}
		return nil
	}()
	switch {
	case tc.expectError != "" && err == nil:
		return out, fmt.Errorf("expected an error containing %q, rendering succeeded", tc.expectError)
	case tc.expectError != "" && !strings.Contains(err.Error(), tc.expectError):
		return out, fmt.Errorf("expected an error containing %q, got: %v", tc.expectError, err)
	case tc.expectError != "":
		return out, nil
	}
	return out, err
}

func keys(m map[string]bool) string {
	var ks []string
	for k := range m {
		ks = append(ks, k)
	}
	sort.Strings(ks)
	return strings.Join(ks, ", ")
}

func deepCopy(v any) any {
	switch t := v.(type) {
	case map[string]any:
		m := make(map[string]any, len(t))
		for k, e := range t {
			m[k] = deepCopy(e)
		}
		return m
	case []any:
		s := make([]any, len(t))
		for i, e := range t {
			s[i] = deepCopy(e)
		}
		return s
	default:
		return v
	}
}

// mergeValues overlays src on dst the way Helm coalesces -f files: maps
// merge recursively, everything else (lists included) is replaced; a null
// in src deletes the key.
func mergeValues(dst, src map[string]any) map[string]any {
	for k, sv := range src {
		if sv == nil {
			delete(dst, k)
			continue
		}
		if sm, ok := sv.(map[string]any); ok {
			if dm, ok := dst[k].(map[string]any); ok {
				dst[k] = mergeValues(dm, sm)
				continue
			}
		}
		dst[k] = sv
	}
	return dst
}

// ---------------------------------------------------------------- render

func render(c *chartFiles, values map[string]any) (manifests, notes string, err error) {
	root := template.New("chart").Option("missingkey=error")
	var includeDepth int
	funcs := sprigSubset()
	funcs["include"] = func(name string, data any) (string, error) {
		includeDepth++
		defer func() { includeDepth-- }()
		if includeDepth > 100 {
			return "", fmt.Errorf("include %q: recursion too deep", name)
		}
		var b bytes.Buffer
		if err := root.ExecuteTemplate(&b, name, data); err != nil {
			return "", err
		}
		return b.String(), nil
	}
	funcs["tpl"] = func(text string, data any) (string, error) {
		t, err := root.Clone()
		if err != nil {
			return "", err
		}
		t, err = t.New("tpl").Parse(text)
		if err != nil {
			return "", err
		}
		var b bytes.Buffer
		err = t.Execute(&b, data)
		return b.String(), err
	}
	root.Funcs(funcs)

	names := make([]string, 0, len(c.templates))
	for n := range c.templates {
		names = append(names, n)
	}
	sort.Strings(names)
	for _, n := range names {
		if _, err := root.New(n).Parse(c.templates[n]); err != nil {
			return "", "", fmt.Errorf("parse %s: %v", n, err)
		}
	}

	chartMeta := map[string]any{
		"Name":       c.meta["name"],
		"Version":    c.meta["version"],
		"AppVersion": c.meta["appVersion"],
	}
	base := path.Join(c.name(), "templates")
	var docs []string
	for _, n := range names {
		file := path.Base(n)
		if strings.HasPrefix(file, "_") {
			continue
		}
		data := map[string]any{
			"Values": values,
			"Chart":  chartMeta,
			"Release": map[string]any{
				"Name":      "wt",
				"Namespace": "worktree",
				"Service":   "Helm",
				"IsInstall": true,
				"IsUpgrade": false,
				"Revision":  1,
			},
			"Template": map[string]any{"Name": n, "BasePath": base},
			"Capabilities": map[string]any{
				"KubeVersion": map[string]any{"Version": "v1.31.0", "Major": "1", "Minor": "31"},
			},
		}
		var b bytes.Buffer
		if err := root.ExecuteTemplate(&b, n, data); err != nil {
			return "", "", fmt.Errorf("render %s: %v", n, err)
		}
		s := strings.ReplaceAll(b.String(), "<no value>", "")
		if file == "NOTES.txt" {
			notes = s
			continue
		}
		if strings.TrimSpace(s) != "" {
			docs = append(docs, "# Source: "+n+"\n"+s)
		}
	}
	return strings.Join(docs, "\n---\n"), notes, nil
}

// ---------------------------------------------------------------- checks

var dns1123 = regexp.MustCompile(`^[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$`)

func checkManifests(manifests string) (map[string]bool, error) {
	seen := map[string]bool{}
	dec := yaml.NewDecoder(strings.NewReader(manifests))
	for i := 0; ; i++ {
		var doc map[string]any
		err := dec.Decode(&doc)
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("document %d: invalid YAML: %v\n%s", i, err, numbered(manifests))
		}
		if doc == nil {
			continue
		}
		doc = normalize(doc).(map[string]any)
		apiVersion, _ := doc["apiVersion"].(string)
		kind, _ := doc["kind"].(string)
		meta, _ := doc["metadata"].(map[string]any)
		name, _ := meta["name"].(string)
		if apiVersion == "" || kind == "" || name == "" {
			return nil, fmt.Errorf("document %d: apiVersion, kind and metadata.name are required", i)
		}
		if len(name) > 253 || !dns1123.MatchString(name) {
			return nil, fmt.Errorf("%s %q: invalid name", kind, name)
		}
		if (kind == "Service" || kind == "StatefulSet") && len(name) > 63 {
			return nil, fmt.Errorf("%s %q: name longer than 63 characters", kind, name)
		}
		id := kind + "/" + name
		if seen[id] {
			return nil, fmt.Errorf("duplicate %s", id)
		}
		seen[id] = true
		if labels, ok := meta["labels"].(map[string]any); ok {
			for k, v := range labels {
				s, ok := v.(string)
				if !ok {
					return nil, fmt.Errorf("%s label %s: value must be a string, got %T", id, k, v)
				}
				if len(s) > 63 {
					return nil, fmt.Errorf("%s label %s: value longer than 63 characters", id, k)
				}
			}
		}
		if err := checkKind(id, kind, doc); err != nil {
			return nil, err
		}
	}
	return seen, nil
}

func checkKind(id, kind string, doc map[string]any) error {
	switch kind {
	case "Secret":
		data, _ := doc["data"].(map[string]any)
		for k, v := range data {
			s, ok := v.(string)
			if !ok {
				return fmt.Errorf("%s data.%s: must be a base64 string", id, k)
			}
			if _, err := base64.StdEncoding.DecodeString(s); err != nil {
				return fmt.Errorf("%s data.%s: invalid base64", id, k)
			}
		}
	case "ConfigMap":
		data, _ := doc["data"].(map[string]any)
		for k, v := range data {
			if _, ok := v.(string); !ok {
				return fmt.Errorf("%s data.%s: value must be a string, got %T", id, k, v)
			}
		}
	case "StatefulSet", "Deployment":
		spec, _ := doc["spec"].(map[string]any)
		sel, _ := dig(spec, "selector", "matchLabels").(map[string]any)
		labels, _ := dig(spec, "template", "metadata", "labels").(map[string]any)
		for k, v := range sel {
			if labels[k] != v {
				return fmt.Errorf("%s: selector %s=%v does not match the pod labels", id, k, v)
			}
		}
		containers, _ := dig(spec, "template", "spec", "containers").([]any)
		if len(containers) == 0 {
			return fmt.Errorf("%s: no containers", id)
		}
		volumes := map[string]bool{}
		if vs, ok := dig(spec, "template", "spec", "volumes").([]any); ok {
			for _, v := range vs {
				if m, ok := v.(map[string]any); ok {
					volumes[fmt.Sprint(m["name"])] = true
				}
			}
		}
		if vcts, ok := spec["volumeClaimTemplates"].([]any); ok {
			for _, v := range vcts {
				volumes[fmt.Sprint(dig(v, "metadata", "name"))] = true
			}
		}
		for _, ci := range containers {
			cm, _ := ci.(map[string]any)
			if img, _ := cm["image"].(string); img == "" || strings.HasSuffix(img, ":") {
				return fmt.Errorf("%s: container %v has no image tag", id, cm["name"])
			}
			for _, probe := range []string{"livenessProbe", "readinessProbe"} {
				if _, ok := cm[probe]; !ok {
					return fmt.Errorf("%s: container %v has no %s", id, cm["name"], probe)
				}
			}
			mounts, _ := cm["volumeMounts"].([]any)
			for _, mi := range mounts {
				name := fmt.Sprint(dig(mi, "name"))
				if !volumes[name] {
					return fmt.Errorf("%s: volumeMount %q has no matching volume", id, name)
				}
			}
		}
	case "Ingress":
		rules, _ := dig(doc, "spec", "rules").([]any)
		if len(rules) == 0 {
			return fmt.Errorf("%s: no rules", id)
		}
	}
	return nil
}

func dig(v any, path ...string) any {
	for _, p := range path {
		m, ok := v.(map[string]any)
		if !ok {
			return nil
		}
		v = m[p]
	}
	return v
}

func numbered(s string) string {
	var b strings.Builder
	for i, l := range strings.Split(s, "\n") {
		fmt.Fprintf(&b, "%4d | %s\n", i+1, l)
	}
	return b.String()
}

// ---------------------------------------------------------------- schema

// validateSchema checks v against the JSON Schema subset used by the chart:
// type, enum, required, properties, items, minimum, maximum, minLength,
// minItems, pattern.
func validateSchema(schema map[string]any, v any, at string) error {
	if types, ok := schema["type"]; ok {
		var list []string
		switch t := types.(type) {
		case string:
			list = []string{t}
		case []any:
			for _, e := range t {
				list = append(list, fmt.Sprint(e))
			}
		}
		matched := false
		for _, t := range list {
			if jsonType(v, t) {
				matched = true
				break
			}
		}
		if !matched {
			return fmt.Errorf("%s: want type %s, got %s", at, strings.Join(list, "|"), describe(v))
		}
	}
	if enum, ok := schema["enum"].([]any); ok {
		found := false
		for _, e := range enum {
			if reflect.DeepEqual(e, v) {
				found = true
				break
			}
		}
		if !found {
			return fmt.Errorf("%s: %v is not one of %v", at, v, enum)
		}
	}
	switch t := v.(type) {
	case map[string]any:
		if req, ok := schema["required"].([]any); ok {
			for _, r := range req {
				if _, ok := t[fmt.Sprint(r)]; !ok {
					return fmt.Errorf("%s: missing required property %v", at, r)
				}
			}
		}
		if props, ok := schema["properties"].(map[string]any); ok {
			for k, ps := range props {
				if pv, ok := t[k]; ok {
					psm, _ := ps.(map[string]any)
					if err := validateSchema(psm, pv, at+"."+k); err != nil {
						return err
					}
				}
			}
		}
	case []any:
		if n, ok := schema["minItems"].(float64); ok && float64(len(t)) < n {
			return fmt.Errorf("%s: want at least %v items", at, n)
		}
		if items, ok := schema["items"].(map[string]any); ok {
			for i, e := range t {
				if err := validateSchema(items, e, fmt.Sprintf("%s[%d]", at, i)); err != nil {
					return err
				}
			}
		}
	case string:
		if n, ok := schema["minLength"].(float64); ok && float64(len(t)) < n {
			return fmt.Errorf("%s: want at least %v characters", at, n)
		}
		if p, ok := schema["pattern"].(string); ok {
			re, err := regexp.Compile(p)
			if err != nil {
				return fmt.Errorf("%s: bad pattern %q in schema: %v", at, p, err)
			}
			if !re.MatchString(t) {
				return fmt.Errorf("%s: %q does not match %s", at, t, p)
			}
		}
	case float64:
		if n, ok := schema["minimum"].(float64); ok && t < n {
			return fmt.Errorf("%s: %v is below the minimum %v", at, t, n)
		}
		if n, ok := schema["maximum"].(float64); ok && t > n {
			return fmt.Errorf("%s: %v is above the maximum %v", at, t, n)
		}
	}
	return nil
}

func jsonType(v any, t string) bool {
	switch t {
	case "object":
		_, ok := v.(map[string]any)
		return ok
	case "array":
		_, ok := v.([]any)
		return ok
	case "string":
		_, ok := v.(string)
		return ok
	case "boolean":
		_, ok := v.(bool)
		return ok
	case "number":
		_, ok := v.(float64)
		return ok
	case "integer":
		f, ok := v.(float64)
		return ok && f == float64(int64(f))
	case "null":
		return v == nil
	}
	return false
}

func describe(v any) string {
	switch v.(type) {
	case nil:
		return "null"
	case map[string]any:
		return "object"
	case []any:
		return "array"
	case string:
		return "string"
	case bool:
		return "boolean"
	case float64:
		return "number"
	}
	return fmt.Sprintf("%T", v)
}

// ---------------------------------------------------------------- functions

// sprigSubset implements the Sprig and Helm template functions used by the
// chart, with Sprig's argument order and semantics.
func sprigSubset() template.FuncMap {
	return template.FuncMap{
		"default": func(def any, given ...any) any {
			if len(given) == 0 || empty(given[0]) {
				return def
			}
			return given[0]
		},
		"empty":      empty,
		"trunc":      func(n int, s string) string { return trunc(n, s) },
		"trimSuffix": func(suffix, s string) string { return strings.TrimSuffix(s, suffix) },
		"trimPrefix": func(prefix, s string) string { return strings.TrimPrefix(s, prefix) },
		"trim":       strings.TrimSpace,
		"contains":   func(substr, s string) bool { return strings.Contains(s, substr) },
		"hasPrefix":  func(prefix, s string) bool { return strings.HasPrefix(s, prefix) },
		"replace":    func(old, repl, s string) string { return strings.ReplaceAll(s, old, repl) },
		"lower":      strings.ToLower,
		"upper":      strings.ToUpper,
		"quote": func(vs ...any) string {
			out := make([]string, 0, len(vs))
			for _, v := range vs {
				if v != nil {
					out = append(out, strconv.Quote(strval(v)))
				}
			}
			return strings.Join(out, " ")
		},
		"squote": func(vs ...any) string {
			out := make([]string, 0, len(vs))
			for _, v := range vs {
				if v != nil {
					out = append(out, "'"+strval(v)+"'")
				}
			}
			return strings.Join(out, " ")
		},
		"toString": strval,
		"int":      func(v any) int { return int(toInt64(v)) },
		"int64":    toInt64,
		"join": func(sep string, v any) string {
			var parts []string
			switch t := v.(type) {
			case []any:
				for _, e := range t {
					parts = append(parts, strval(e))
				}
			case []string:
				parts = t
			default:
				parts = []string{strval(v)}
			}
			return strings.Join(parts, sep)
		},
		"indent": func(n int, s string) string {
			pad := strings.Repeat(" ", n)
			return pad + strings.ReplaceAll(s, "\n", "\n"+pad)
		},
		"nindent": func(n int, s string) string {
			pad := strings.Repeat(" ", n)
			return "\n" + pad + strings.ReplaceAll(s, "\n", "\n"+pad)
		},
		"toYaml": func(v any) (string, error) {
			var b bytes.Buffer
			enc := yaml.NewEncoder(&b)
			enc.SetIndent(2)
			if err := enc.Encode(v); err != nil {
				return "", err
			}
			_ = enc.Close()
			return strings.TrimSuffix(b.String(), "\n"), nil
		},
		"toJson": func(v any) (string, error) {
			b, err := json.Marshal(v)
			return string(b), err
		},
		"b64enc": func(v any) string { return base64.StdEncoding.EncodeToString([]byte(strval(v))) },
		"b64dec": func(v any) (string, error) {
			b, err := base64.StdEncoding.DecodeString(strval(v))
			return string(b), err
		},
		"sha256sum": func(s string) string {
			h := sha256.Sum256([]byte(s))
			return hex.EncodeToString(h[:])
		},
		"randAlphaNum": func(n int) (string, error) {
			const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
			b := make([]byte, n)
			for i := range b {
				k, err := rand.Int(rand.Reader, big.NewInt(int64(len(alphabet))))
				if err != nil {
					return "", err
				}
				b[i] = alphabet[k.Int64()]
			}
			return string(b), nil
		},
		"hasKey": func(m map[string]any, k string) bool { _, ok := m[k]; return ok },
		"dict": func(kv ...any) (map[string]any, error) {
			if len(kv)%2 != 0 {
				return nil, errors.New("dict: odd number of arguments")
			}
			m := map[string]any{}
			for i := 0; i < len(kv); i += 2 {
				m[strval(kv[i])] = kv[i+1]
			}
			return m, nil
		},
		"list": func(vs ...any) []any { return vs },
		// merge: earlier arguments win (Sprig/mergo semantics); returns dst.
		"merge": func(dst map[string]any, srcs ...map[string]any) map[string]any {
			for _, src := range srcs {
				mergeMissing(dst, src)
			}
			return dst
		},
		"ternary": func(t, f any, cond bool) any {
			if cond {
				return t
			}
			return f
		},
		"required": func(msg string, v any) (any, error) {
			if empty(v) {
				return nil, errors.New(msg)
			}
			return v, nil
		},
		"fail": func(msg string) (string, error) { return "", errors.New(msg) },
		// lookup never finds anything offline (like `helm template`).
		"lookup": func(apiVersion, kind, namespace, name string) map[string]any {
			return map[string]any{}
		},
	}
}

func mergeMissing(dst, src map[string]any) {
	for k, sv := range src {
		dv, ok := dst[k]
		if !ok {
			dst[k] = deepCopy(sv)
			continue
		}
		dm, dok := dv.(map[string]any)
		sm, sok := sv.(map[string]any)
		if dok && sok {
			mergeMissing(dm, sm)
		}
	}
}

func empty(v any) bool {
	if v == nil {
		return true
	}
	rv := reflect.ValueOf(v)
	switch rv.Kind() {
	case reflect.String, reflect.Map, reflect.Slice, reflect.Array:
		return rv.Len() == 0
	case reflect.Bool:
		return !rv.Bool()
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return rv.Int() == 0
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return rv.Uint() == 0
	case reflect.Float32, reflect.Float64:
		return rv.Float() == 0
	case reflect.Pointer, reflect.Interface:
		return rv.IsNil()
	}
	return false
}

func strval(v any) string {
	switch t := v.(type) {
	case string:
		return t
	case []byte:
		return string(t)
	case nil:
		return ""
	case float64:
		return strconv.FormatFloat(t, 'f', -1, 64)
	case error:
		return t.Error()
	case fmt.Stringer:
		return t.String()
	default:
		return fmt.Sprint(v)
	}
}

func toInt64(v any) int64 {
	switch t := v.(type) {
	case int:
		return int64(t)
	case int64:
		return t
	case float64:
		return int64(t)
	case string:
		n, _ := strconv.ParseInt(t, 10, 64)
		return n
	case bool:
		if t {
			return 1
		}
	}
	return 0
}

func trunc(n int, s string) string {
	if n >= 0 && len(s) > n {
		return s[:n]
	}
	if n < 0 && len(s) > -n {
		return s[len(s)+n:]
	}
	return s
}
