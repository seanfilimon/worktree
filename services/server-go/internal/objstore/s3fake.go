package objstore

// FakeS3 is an in-memory S3 API (path-style requests) implementing the
// subset S3Store uses: HeadBucket, CreateBucket, GetBucketLocation,
// PutObject, GetObject, HeadObject, DeleteObject and ListObjectsV2 with
// pagination. It exists for tests (serve it with net/http/httptest);
// requests are not signature-checked beyond the access key named in the
// Authorization header.

import (
	"crypto/md5"
	"encoding/base64"
	"encoding/hex"
	"encoding/xml"
	"fmt"
	"io"
	"net/http"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// FakeS3 is an http.Handler emulating an S3 endpoint in memory.
type FakeS3 struct {
	// AccessKey, when set, is required in the Authorization header
	// ("Credential=<AccessKey>/..."); other requests get 403 AccessDenied.
	AccessKey string
	// PageSize caps ListObjectsV2 pages (0 = 1000), to exercise
	// continuation tokens with few objects.
	PageSize int

	mu      sync.Mutex
	buckets map[string]map[string]fakeObject

	requests atomic.Int64
	puts     atomic.Int64
	// failNext makes the next n object requests answer 500 InternalError.
	failNext atomic.Int64
}

type fakeObject struct {
	data     []byte
	etag     string
	modified time.Time
}

// NewFakeS3 returns an empty fake with no buckets.
func NewFakeS3() *FakeS3 {
	return &FakeS3{buckets: map[string]map[string]fakeObject{}}
}

// CreateBucket creates bucket (no-op when it exists).
func (f *FakeS3) CreateBucket(bucket string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.buckets[bucket] == nil {
		f.buckets[bucket] = map[string]fakeObject{}
	}
}

// Keys returns the sorted keys of bucket.
func (f *FakeS3) Keys(bucket string) []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]string, 0, len(f.buckets[bucket]))
	for k := range f.buckets[bucket] {
		out = append(out, k)
	}
	slices.Sort(out)
	return out
}

// Object returns a copy of the stored bytes of key.
func (f *FakeS3) Object(bucket, key string) ([]byte, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	o, ok := f.buckets[bucket][key]
	return slices.Clone(o.data), ok
}

// SetObject stores data under key (creating the bucket), bypassing HTTP.
func (f *FakeS3) SetObject(bucket, key string, data []byte) {
	f.CreateBucket(bucket)
	f.mu.Lock()
	defer f.mu.Unlock()
	f.buckets[bucket][key] = newFakeObject(data)
}

// Requests returns the number of requests served.
func (f *FakeS3) Requests() int64 { return f.requests.Load() }

// Puts returns the number of PutObject requests served.
func (f *FakeS3) Puts() int64 { return f.puts.Load() }

// FailNext makes the next n object requests fail with 500 InternalError.
func (f *FakeS3) FailNext(n int) { f.failNext.Store(int64(n)) }

func newFakeObject(data []byte) fakeObject {
	sum := md5.Sum(data)
	return fakeObject{data: slices.Clone(data), etag: hex.EncodeToString(sum[:]), modified: time.Now().UTC().Truncate(time.Second)}
}

type s3Error struct {
	XMLName    xml.Name `xml:"Error"`
	Code       string   `xml:"Code"`
	Message    string   `xml:"Message"`
	BucketName string   `xml:"BucketName,omitempty"`
	Key        string   `xml:"Key,omitempty"`
	RequestID  string   `xml:"RequestId"`
}

func (f *FakeS3) fail(w http.ResponseWriter, r *http.Request, status int, code, msg, bucket, key string) {
	w.Header().Set("Content-Type", "application/xml")
	w.Header().Set("x-amz-request-id", "fake")
	w.WriteHeader(status)
	if r.Method == http.MethodHead {
		return
	}
	_ = xml.NewEncoder(w).Encode(s3Error{Code: code, Message: msg, BucketName: bucket, Key: key, RequestID: "fake"})
}

func (f *FakeS3) writeXML(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/xml")
	_, _ = io.WriteString(w, xml.Header)
	_ = xml.NewEncoder(w).Encode(v)
}

// ServeHTTP implements http.Handler.
func (f *FakeS3) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	f.requests.Add(1)
	if f.AccessKey != "" && !strings.Contains(r.Header.Get("Authorization"), "Credential="+f.AccessKey+"/") {
		f.fail(w, r, http.StatusForbidden, "AccessDenied", "Access Denied.", "", "")
		return
	}
	bucket, key, _ := strings.Cut(strings.TrimPrefix(r.URL.Path, "/"), "/")
	if bucket == "" {
		f.fail(w, r, http.StatusNotImplemented, "NotImplemented", "ListBuckets is not implemented", "", "")
		return
	}
	if key == "" {
		f.serveBucket(w, r, bucket)
		return
	}
	if n := f.failNext.Load(); n > 0 && f.failNext.CompareAndSwap(n, n-1) {
		f.fail(w, r, http.StatusInternalServerError, "InternalError", "injected failure", bucket, key)
		return
	}
	f.serveObject(w, r, bucket, key)
}

func (f *FakeS3) serveBucket(w http.ResponseWriter, r *http.Request, bucket string) {
	q := r.URL.Query()
	f.mu.Lock()
	objs, exists := f.buckets[bucket]
	f.mu.Unlock()
	switch {
	case r.Method == http.MethodPut:
		f.CreateBucket(bucket)
		w.Header().Set("Location", "/"+bucket)
		w.WriteHeader(http.StatusOK)
	case !exists:
		f.fail(w, r, http.StatusNotFound, "NoSuchBucket", "The specified bucket does not exist", bucket, "")
	case r.Method == http.MethodHead:
		w.WriteHeader(http.StatusOK)
	case r.Method == http.MethodGet && q.Has("location"):
		f.writeXML(w, struct {
			XMLName xml.Name `xml:"http://s3.amazonaws.com/doc/2006-03-01/ LocationConstraint"`
			Value   string   `xml:",chardata"`
		}{})
	case r.Method == http.MethodGet && q.Get("list-type") == "2":
		f.listV2(w, r, bucket, objs)
	default:
		f.fail(w, r, http.StatusNotImplemented, "NotImplemented", r.Method+" "+r.URL.RawQuery+" is not implemented", bucket, "")
	}
}

type listContents struct {
	Key          string `xml:"Key"`
	LastModified string `xml:"LastModified"`
	ETag         string `xml:"ETag"`
	Size         int64  `xml:"Size"`
	StorageClass string `xml:"StorageClass"`
}

type listResult struct {
	XMLName               xml.Name       `xml:"http://s3.amazonaws.com/doc/2006-03-01/ ListBucketResult"`
	Name                  string         `xml:"Name"`
	Prefix                string         `xml:"Prefix"`
	KeyCount              int            `xml:"KeyCount"`
	MaxKeys               int            `xml:"MaxKeys"`
	IsTruncated           bool           `xml:"IsTruncated"`
	ContinuationToken     string         `xml:"ContinuationToken,omitempty"`
	NextContinuationToken string         `xml:"NextContinuationToken,omitempty"`
	StartAfter            string         `xml:"StartAfter,omitempty"`
	Contents              []listContents `xml:"Contents"`
}

func (f *FakeS3) listV2(w http.ResponseWriter, r *http.Request, bucket string, objs map[string]fakeObject) {
	q := r.URL.Query()
	prefix := q.Get("prefix")
	maxKeys := 1000
	if s := q.Get("max-keys"); s != "" {
		if n, err := strconv.Atoi(s); err == nil && n >= 0 && n < maxKeys {
			maxKeys = n
		}
	}
	if f.PageSize > 0 && f.PageSize < maxKeys {
		maxKeys = f.PageSize
	}
	after := q.Get("start-after")
	token := q.Get("continuation-token")
	if token != "" {
		b, err := base64.StdEncoding.DecodeString(token)
		if err != nil {
			f.fail(w, r, http.StatusBadRequest, "InvalidArgument", "The continuation token provided is incorrect", bucket, "")
			return
		}
		after = string(b)
	}

	f.mu.Lock()
	var keys []string
	for k := range objs {
		if strings.HasPrefix(k, prefix) && k > after {
			keys = append(keys, k)
		}
	}
	slices.Sort(keys)
	res := listResult{Name: bucket, Prefix: prefix, MaxKeys: maxKeys, ContinuationToken: token, StartAfter: q.Get("start-after")}
	if len(keys) > maxKeys {
		res.IsTruncated = true
		keys = keys[:maxKeys]
		res.NextContinuationToken = base64.StdEncoding.EncodeToString([]byte(keys[len(keys)-1]))
	}
	for _, k := range keys {
		o := objs[k]
		res.Contents = append(res.Contents, listContents{
			Key: k, LastModified: o.modified.Format("2006-01-02T15:04:05.000Z"), ETag: `"` + o.etag + `"`,
			Size: int64(len(o.data)), StorageClass: "STANDARD",
		})
	}
	f.mu.Unlock()
	res.KeyCount = len(res.Contents)
	f.writeXML(w, res)
}

func (f *FakeS3) serveObject(w http.ResponseWriter, r *http.Request, bucket, key string) {
	f.mu.Lock()
	objs, exists := f.buckets[bucket]
	o, found := objs[key]
	f.mu.Unlock()
	if !exists {
		f.fail(w, r, http.StatusNotFound, "NoSuchBucket", "The specified bucket does not exist", bucket, key)
		return
	}
	switch r.Method {
	case http.MethodPut:
		if r.Header.Get("x-amz-copy-source") != "" {
			f.fail(w, r, http.StatusNotImplemented, "NotImplemented", "CopyObject is not implemented", bucket, key)
			return
		}
		data, err := io.ReadAll(r.Body)
		if err != nil {
			f.fail(w, r, http.StatusBadRequest, "IncompleteBody", err.Error(), bucket, key)
			return
		}
		if want := r.Header.Get("Content-MD5"); want != "" {
			sum := md5.Sum(data)
			if base64.StdEncoding.EncodeToString(sum[:]) != want {
				f.fail(w, r, http.StatusBadRequest, "BadDigest", "The Content-MD5 you specified did not match what we received.", bucket, key)
				return
			}
		}
		obj := newFakeObject(data)
		f.mu.Lock()
		f.buckets[bucket][key] = obj
		f.mu.Unlock()
		f.puts.Add(1)
		w.Header().Set("ETag", `"`+obj.etag+`"`)
		w.WriteHeader(http.StatusOK)
	case http.MethodGet, http.MethodHead:
		if !found {
			f.fail(w, r, http.StatusNotFound, "NoSuchKey", "The specified key does not exist.", bucket, key)
			return
		}
		h := w.Header()
		h.Set("Content-Type", "application/octet-stream")
		h.Set("Content-Length", strconv.Itoa(len(o.data)))
		h.Set("ETag", `"`+o.etag+`"`)
		h.Set("Last-Modified", o.modified.Format(http.TimeFormat))
		h.Set("Accept-Ranges", "bytes")
		w.WriteHeader(http.StatusOK)
		if r.Method == http.MethodGet {
			_, _ = w.Write(o.data)
		}
	case http.MethodDelete:
		f.mu.Lock()
		delete(f.buckets[bucket], key)
		f.mu.Unlock()
		w.WriteHeader(http.StatusNoContent)
	default:
		f.fail(w, r, http.StatusMethodNotAllowed, "MethodNotAllowed", fmt.Sprintf("%s is not allowed", r.Method), bucket, key)
	}
}
