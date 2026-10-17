# Cloud Provider Examples

This directory contains XCL examples for testing all 8 cloud providers with the unified service naming convention.

## Providers

| Provider | File | Context Parameter | Services |
|---|---|---|---|
| AWS | `aws.xcl` | Region (`us-east-1`) | bucket (S3), function (Lambda), api (API Gateway) |
| GCP | `gcp.xcl` | Project (`my-project`) | bucket (Cloud Storage), function (Cloud Functions), instance (Compute Engine) |
| Azure | `azure.xcl` | Subscription (`my-subscription`) | bucket (Blob Storage), function (Azure Functions), instance (Azure VM) |
| Alibaba | `alibaba.xcl` | Account (`my-account`) | bucket (OSS), function (FC), instance (ECS) |
| IBM | `ibm.xcl` | Account (`my-account`) | bucket (COS), function (Cloud Functions), instance (VPC) |
| OCI | `oci.xcl` | Tenancy (`my-tenancy`) | bucket (Object Storage), function (OCI Functions), instance (Compute) |
| Cloudflare | `cloudflare.xcl` | Account (`my-account`) | bucket (R2), function (Workers), instance (Durable Objects) |
| GitHub | `github.xcl` | Organization (`my-org`) | bucket (Packages), function (Actions), instance (Runners) |

## Unified Service Names

All providers use the same service names regardless of the underlying platform:

- **`bucket`** — Object storage (S3, Cloud Storage, Blob Storage, OSS, COS, R2, etc.)
- **`function`** — Serverless functions (Lambda, Cloud Functions, Azure Functions, Workers, etc.)
- **`instance`** — Compute instances (EC2, GCE, Azure VM, ECS, VPC, etc.)
- **`api`** — API gateways (AWS API Gateway, etc.)

## Domain Expansion Pattern

Each provider uses domain expansion for clean, scoped operations:

```xcl
aws "us-east-1" {
    bucket "my-bucket" {
        $list = bucket.list ""
        $data = bucket.get "key.txt"
    }
    function "my-function" {
        $result = function.invoke { input: "test" }
    }
}
```

## Running Examples

```bash
# Run a single provider example
xcl example/cloud/aws.xcl

# Run all examples
for file in example/cloud/*.xcl; do
    xcl "$file"
done
```

## Prerequisites

To run with real cloud resources, set the appropriate environment variables:

### AWS
- `AWS_ACCESS_KEY_ID` — Your AWS access key
- `AWS_SECRET_ACCESS_KEY` — Your AWS secret key
- `AWS_REGION` — Default region (optional)

### GCP
- `GOOGLE_APPLICATION_CREDENTIALS` — Path to service account JSON key
- Or set `GCP_ENV_FILE` to point to a `.env` file

### Azure
- `AZURE_CLIENT_ID` — Azure AD application client ID
- `AZURE_CLIENT_SECRET` — Azure AD application secret
- `AZURE_TENANT_ID` — Azure AD tenant ID

### Alibaba
- `ALIBABA_ACCESS_KEY_ID` — Alibaba access key
- `ALIBABA_ACCESS_KEY_SECRET` — Alibaba secret key

### IBM
- `IBM_API_KEY` — IBM Cloud API key

### OCI
- `OCI_USER_ID` — OCI user OCID
- `OCI_TENANCY_ID` — OCI tenancy OCID
- `OCI_FINGERPRINT` — API key fingerprint
- `OCI_PRIVATE_KEY_PATH` — Path to private key file

### Cloudflare
- `CLOUDFLARE_API_TOKEN` — Cloudflare API token (preferred)
- Or `CLOUDFLARE_API_KEY` + `CLOUDFLARE_EMAIL`

### GitHub
- `GITHUB_TOKEN` — GitHub personal access token

## Expected Output

Without real credentials, the examples will print error messages for service operations:

```
AWS region: us-east-1
AWS credentials loaded
Bucket list error (expected without real credentials): missing_credentials
Function invoke error (expected without real credentials): missing_credentials
API create error (expected without real credentials): missing_credentials
Nested region: eu-west-1
AWS example complete
```

With valid credentials, the examples will perform real operations against your cloud provider.
