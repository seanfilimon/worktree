package objstore

// server-ops (docs/ENGINEERING_PLAN.md §16.11): a plain S3 bucket handle
// for data that is not content-addressed (backup archives), built from the
// same S3Options as the object store so `--backup-target s3` reuses the
// `--s3-*` configuration.

import (
	"context"
	"errors"
	"fmt"
	"net/http"

	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"
)

// S3Bucket is a connected S3 client bound to one bucket.
type S3Bucket struct {
	// Client is the underlying minio client.
	Client *minio.Client
	// Bucket is the bucket name; Endpoint host[:port]; Secure reports HTTPS.
	Bucket   string
	Endpoint string
	Secure   bool
	Region   string
}

// NewS3Bucket connects to the bucket described by o (Prefix and Compress
// are ignored), creating it when missing, like NewS3.
func NewS3Bucket(ctx context.Context, o S3Options) (*S3Bucket, error) {
	endpoint, secure, err := parseS3Endpoint(o.Endpoint, o.Insecure)
	if err != nil {
		return nil, err
	}
	if o.Bucket == "" {
		return nil, errors.New("objstore: s3: bucket is required")
	}
	var creds *credentials.Credentials
	if o.AccessKey != "" || o.SecretKey != "" {
		creds = credentials.NewStaticV4(o.AccessKey, o.SecretKey, "")
	} else {
		creds = credentials.NewChainCredentials([]credentials.Provider{
			&credentials.EnvAWS{},
			&credentials.EnvMinio{},
			&credentials.IAM{Client: &http.Client{Transport: http.DefaultTransport}},
		})
	}
	client, err := minio.New(endpoint, &minio.Options{
		Creds: creds, Secure: secure, Region: o.Region, Transport: o.Transport, MaxRetries: o.MaxRetries,
	})
	if err != nil {
		return nil, fmt.Errorf("objstore: s3: %w", err)
	}
	exists, err := client.BucketExists(ctx, o.Bucket)
	if err != nil {
		return nil, fmt.Errorf("objstore: s3: check bucket %q on %s: %w", o.Bucket, endpoint, err)
	}
	if !exists {
		if err := client.MakeBucket(ctx, o.Bucket, minio.MakeBucketOptions{Region: o.Region}); err != nil {
			if again, aerr := client.BucketExists(ctx, o.Bucket); aerr != nil || !again {
				return nil, fmt.Errorf("objstore: s3: create bucket %q on %s: %w", o.Bucket, endpoint, err)
			}
		}
	}
	return &S3Bucket{Client: client, Bucket: o.Bucket, Endpoint: endpoint, Secure: secure, Region: o.Region}, nil
}

// NormalizeS3Prefix trims leading slashes and adds a trailing one ("" stays
// ""), the form S3Store uses for --s3-prefix.
func NormalizeS3Prefix(p string) string { return normalizePrefix(p) }

// IsS3NotFound reports whether err is S3's "no such key".
func IsS3NotFound(err error) bool { return isNoSuchKey(err) }
