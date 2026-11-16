//! Exercise the canonical read-only reconciler against real PostgreSQL 16.
use super::{
    Cluster, OPERATION_ID, Result, Tools, failure, psql, query, run_script,
    validate_restored_records,
};
use std::path::Path;

pub(super) async fn rehearse(
    tools: &Tools,
    cluster: &Cluster,
    project: &Path,
    environment: &[(&str, &str)],
    previous: &str,
) -> Result<()> {
    // A completed restore has both a durable receipt and its previous seed.
    run_script(tools, project, "reconcile-seed.sh", environment).await?;
    let stage = format!("seed_stage_{}", OPERATION_ID.replace('-', ""));
    psql(
        tools,
        cluster,
        "postgres",
        &format!("CREATE DATABASE {stage}"),
    )
    .await?;
    require_unknown(tools, project, environment).await?;
    psql(
        tools,
        cluster,
        "postgres",
        &format!("DROP DATABASE {stage}"),
    )
    .await?;

    // A first installation has no previous seed: only the live receipt proves it.
    psql(
        tools,
        cluster,
        "postgres",
        &format!("ALTER DATABASE {previous} RENAME TO rehearsal_hidden_previous"),
    )
    .await?;
    run_script(tools, project, "reconcile-seed.sh", environment).await?;
    psql(
        tools,
        cluster,
        "postgres",
        "COMMENT ON DATABASE seed IS NULL",
    )
    .await?;
    require_unknown(tools, project, environment).await?;

    // Older releases lack receipts; their completed rename topology still proves
    // completion, but a receipt belonging to a different operation never does.
    psql(
        tools,
        cluster,
        "postgres",
        &format!("ALTER DATABASE rehearsal_hidden_previous RENAME TO {previous}"),
    )
    .await?;
    run_script(tools, project, "reconcile-seed.sh", environment).await?;
    psql(
        tools,
        cluster,
        "postgres",
        "COMMENT ON DATABASE seed IS 'wrong-operation'",
    )
    .await?;
    require_unknown(tools, project, environment).await?;
    let receipt =
        format!("backstage-aws-seed-v1:{OPERATION_ID}:operations/seed/{OPERATION_ID}.dump");
    psql(
        tools,
        cluster,
        "postgres",
        &format!("COMMENT ON DATABASE seed IS '{receipt}'"),
    )
    .await?;
    run_script(tools, project, "reconcile-seed.sh", environment).await?;
    validate_restored_records(&query(tools, cluster, "seed").await?)
}

async fn require_unknown(
    tools: &Tools,
    project: &Path,
    environment: &[(&str, &str)],
) -> Result<()> {
    if run_script(tools, project, "reconcile-seed.sh", environment)
        .await
        .is_ok()
    {
        return Err(failure(
            "local runtime",
            "ambiguous seed outcome was accepted",
        ));
    }
    Ok(())
}
