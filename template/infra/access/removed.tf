# The workload boundary moved to bootstrap, which must create it before the GitHub
# deploy role attaches it. This block forgets the old state entry without deleting
# the live policy that existing roles may still reference.
removed {
  from = aws_iam_policy.workload_boundary
  lifecycle {
    destroy = false
  }
}
