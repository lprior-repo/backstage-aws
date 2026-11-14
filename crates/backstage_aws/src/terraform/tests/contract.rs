//! Output names shared by the CLI, the `env` stack, and the `base` stack. A
//! renamed output would otherwise only surface as a failed deployment.
const BASE_OUTPUTS: &str = include_str!("../../../../../template/infra/base/outputs.tf");
const ENV_OUTPUTS: &str = include_str!("../../../../../template/infra/env/outputs.tf");
const ENV_MAIN: &str = include_str!("../../../../../template/infra/env/main.tf");

fn declared(outputs: &'static str) -> Vec<&'static str> {
    let name = |line: &'static str| line.strip_prefix("output \"")?.split('"').next();
    outputs.lines().filter_map(name).collect()
}

#[test]
fn the_stacks_declare_every_output_read_from_them() {
    let base = declared(BASE_OUTPUTS);
    let env = declared(ENV_OUTPUTS);
    let cli_reads_from_base = [
        "ecs_cluster_name",
        "ecr_repository_url",
        "ecr_repository_name",
        "seed_bucket_name",
        "database_application_secret_arn",
        "identity_secret_arn",
        "db_admin_init_task_definition_arn",
        "db_admin_rotate_task_definition_arn",
        "db_admin_seed_task_definition_arn",
        "db_admin_seed_restore_task_definition_arn",
        "db_admin_network_configuration",
    ];
    let env_reads_from_base = [ENV_MAIN, ENV_OUTPUTS].into_iter().flat_map(|source| {
        let name = |rest: &'static str| {
            rest.split(|c: char| c != '_' && !c.is_ascii_alphanumeric())
                .next()
        };
        source.split("local.base.").skip(1).filter_map(name)
    });
    for name in cli_reads_from_base.into_iter().chain(env_reads_from_base) {
        assert!(base.contains(&name), "base stack does not output `{name}`");
    }
    for name in [
        "url",
        "image_digest",
        "ecs_cluster_name",
        "ecs_service_name",
        "task_definition_arn",
        "target_group_arn",
    ] {
        assert!(env.contains(&name), "env stack does not output `{name}`");
    }
}
