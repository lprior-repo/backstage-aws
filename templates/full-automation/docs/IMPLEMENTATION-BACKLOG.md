# Acceptance backlog

The automation is implemented; these environment acceptance items remain editable and unverified until exercised in your account.

| Item | State | Evidence to retain |
| --- | --- | --- |
| Real account/root MFA, SSO and delegated DNS | unknown | Account identity and administrator validation |
| Terraform provider validation/mock plans on a normal host | blocked in preparation environment | scripts/check-automation.sh result |
| Full image build and app/DB integration tests | needs verification | CI and container test output |
| GitHub OIDC environment/reviewer/branch enforcement | needs verification | Environment settings and role trust evaluation |
| Actual AWS ASL validation of rollout and maintenance | needs verification | API diagnostic files |
| First real deployment and secret injection | unknown | Execution ARN, task revision, scan, DB task exit, live checks |
| Real GitHub sign-in and default-deny permissions | unknown | Operator/nonoperator test record |
| Secret and database rotation failure/recovery rehearsal | unknown | Version IDs and execution/task evidence; no values |
| SNS email confirmation and real alert delivery | unknown | Confirmed subscription plus test notification record |
| Database restore, app compatibility and cutover rehearsal | unknown | Snapshot/restore IDs, data checks, RTO/RPO |
| Workload-specific sizing, egress policy and cost acceptance | unknown | Measurements and reviewed configuration |

Potential extensions, only if justified: public-edge WAF/SSO proxy, distributed tracing, signed/attested image promotion, multi-account organization onboarding and a dedicated EC2 compute implementation. None is claimed as shipped or tested by this bundle.
