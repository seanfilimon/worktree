package httpapi

import (
	"fmt"
	"html"
	"net/http"
	"net/http/pprof"
	"runtime"
	runtimepprof "runtime/pprof"
	"strings"
)

// Runtime profiling (docs/ENGINEERING_PLAN.md §15.10, workstream
// hardening). With --pprof (env WT_PPROF) server admins can fetch the Go
// runtime profiles; without it both routes answer 404 not_found, exactly
// like an unknown path, so the endpoints do not advertise themselves.

// pprofProfiles are the profiles served under /debug/pprof/{profile}, in
// index order, with a one-line description each.
var pprofProfiles = []struct{ name, desc string }{
	{"allocs", "A sampling of all past memory allocations."},
	{"block", "Stack traces that led to blocking on synchronization primitives (empty unless the block profile rate is set)."},
	{"cmdline", "The command line invocation of the server."},
	{"goroutine", "Stack traces of all current goroutines (?debug=1 or ?debug=2 for text)."},
	{"heap", "A sampling of memory allocations of live objects (?gc=1 runs a GC first)."},
	{"mutex", "Stack traces of holders of contended mutexes (empty unless the mutex profile fraction is set)."},
	{"profile", "CPU profile; ?seconds=N (default 30)."},
	{"symbol", "Looks up the program counters listed in the query (?0x1234+0x5678)."},
	{"threadcreate", "Stack traces that led to the creation of new OS threads."},
	{"trace", "Execution trace; ?seconds=N (default 1)."},
}

// registerPprofRoutes registers GET /debug/pprof and
// GET /debug/pprof/{profile} (hardening, §15.10).
func (a *api) registerPprofRoutes() {
	a.handle("GET /debug/pprof", a.handlePprofIndex)
	a.handle("GET /debug/pprof/{profile}", a.handlePprofProfile)
}

// pprofGate answers 404 when profiling is off, else requires a server admin.
func (a *api) pprofGate(r *http.Request) error {
	if !a.d.Pprof {
		return notFoundf("no such endpoint: %s %s", r.Method, r.URL.Path)
	}
	return a.requireServerAdmin(r)
}

// handlePprofIndex answers GET /debug/pprof: an HTML page linking every
// profile by absolute path (the route has no trailing slash, so relative
// links would resolve against /debug/).
func (a *api) handlePprofIndex(w http.ResponseWriter, r *http.Request) error {
	if err := a.pprofGate(r); err != nil {
		return err
	}
	var b strings.Builder
	b.WriteString("<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>/debug/pprof</title></head><body>\n")
	fmt.Fprintf(&b, "<h1>/debug/pprof</h1>\n<p>%d goroutines, GOMAXPROCS %d, %s</p>\n<table>\n",
		runtime.NumGoroutine(), runtime.GOMAXPROCS(0), html.EscapeString(runtime.Version()))
	b.WriteString("<tr><th>Count</th><th>Profile</th><th>Description</th></tr>\n")
	for _, p := range pprofProfiles {
		count := ""
		if prof := runtimepprof.Lookup(p.name); prof != nil {
			count = fmt.Sprint(prof.Count())
		}
		href := "/debug/pprof/" + p.name
		if p.name == "goroutine" {
			href += "?debug=1"
		}
		fmt.Fprintf(&b, "<tr><td>%s</td><td><a href=\"%s\">%s</a></td><td>%s</td></tr>\n",
			count, html.EscapeString(href), p.name, html.EscapeString(p.desc))
	}
	b.WriteString("</table>\n<p>Use <code>go tool pprof http://HOST/debug/pprof/heap</code> with a bearer token header, or download and inspect locally.</p>\n</body></html>\n")
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	_, _ = w.Write([]byte(b.String()))
	return nil
}

// handlePprofProfile answers GET /debug/pprof/{profile} with the
// net/http/pprof handler of that profile.
func (a *api) handlePprofProfile(w http.ResponseWriter, r *http.Request) error {
	if err := a.pprofGate(r); err != nil {
		return err
	}
	name := r.PathValue("profile")
	w.Header().Set("Cache-Control", "no-store")
	switch name {
	case "cmdline":
		pprof.Cmdline(w, r)
	case "profile":
		pprof.Profile(w, r)
	case "symbol":
		pprof.Symbol(w, r)
	case "trace":
		pprof.Trace(w, r)
	case "allocs", "block", "goroutine", "heap", "mutex", "threadcreate":
		pprof.Handler(name).ServeHTTP(w, r)
	default:
		return notFoundf("unknown profile %q", name)
	}
	return nil
}
