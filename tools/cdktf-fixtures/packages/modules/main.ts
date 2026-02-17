import { App, TerraformStack, TerraformOutput, TerraformVariable, TerraformHclModule, Fn } from "cdktf";
import { Construct } from "constructs";
import { provider as randomProvider, pet, stringResource } from "@cdktf/provider-random";
import { provider as nullProvider, resource as nullResource } from "@cdktf/provider-null";
import { provider as localProvider, file } from "@cdktf/provider-local";

/**
 * Demonstrates module invocation patterns in tf.json:
 * - TerraformHclModule for referencing external Terraform modules
 * - Nested construct composition (CDKTF-native module pattern)
 * - Module outputs and cross-module references
 */

// CDKTF construct acting as a reusable "module" — generates inline resources
class NamingModule extends Construct {
  public readonly petName: pet.Pet;
  public readonly token: stringResource.StringResource;

  constructor(scope: Construct, id: string, props: { prefix: string; tokenLength: number }) {
    super(scope, id);

    this.petName = new pet.Pet(this, "name", {
      prefix: props.prefix,
      length: 2,
    });

    this.token = new stringResource.StringResource(this, "token", {
      length: props.tokenLength,
      special: false,
      upper: false,
    });
  }
}

class ModulesStack extends TerraformStack {
  constructor(scope: Construct, id: string) {
    super(scope, id);

    new randomProvider.RandomProvider(this, "random");
    new nullProvider.NullProvider(this, "null");
    new localProvider.LocalProvider(this, "local");

    const projectName = new TerraformVariable(this, "project_name", {
      type: "string",
      default: "oxid-test",
    });

    const environment = new TerraformVariable(this, "environment", {
      type: "string",
      default: "dev",
    });

    // Pattern 1: CDKTF construct composition (generates inline resources)
    const naming = new NamingModule(this, "naming", {
      prefix: projectName.stringValue,
      tokenLength: 24,
    });

    // Pattern 2: TerraformHclModule — references a Terraform registry module
    // Using hashicorp/dir/template which needs no credentials
    const configTemplates = new TerraformHclModule(this, "config_templates", {
      source: "hashicorp/dir/template",
      version: "1.0.2",
      variables: {
        base_dir: "${path.module}/templates",
      },
    });

    // Pattern 3: TerraformHclModule — local module path reference
    const network = new TerraformHclModule(this, "network_config", {
      source: "./modules/network",
      skipAssetCreationFromLocalModules: true,
      variables: {
        project: projectName.stringValue,
        environment: environment.stringValue,
        cidr_block: "10.0.0.0/16",
        availability_zones: ["us-east-1a", "us-east-1b", "us-east-1c"],
      },
    });

    // Pattern 4: Module with depends_on
    const appConfig = new TerraformHclModule(this, "app_config", {
      source: "./modules/app",
      skipAssetCreationFromLocalModules: true,
      variables: {
        name: projectName.stringValue,
        network_id: network.getString("vpc_id"),
        subnet_ids: network.get("subnet_ids"),
      },
      dependsOn: [network],
    });

    // Resource referencing module outputs
    new file.File(this, "manifest", {
      filename: "${path.module}/output/manifest.json",
      content: Fn.jsonencode({
        project: projectName.stringValue,
        pet_name: naming.petName.id,
        token: naming.token.result,
        config_template_files: configTemplates.get("files"),
      }),
    });

    // Null resource depending on the module
    new nullResource.Resource(this, "provisioner", {
      triggers: {
        app_id: appConfig.getString("app_id"),
        manifest: "${local_file.manifest.content}",
      },
      dependsOn: [appConfig],
    });

    // Outputs
    new TerraformOutput(this, "pet_name", {
      value: naming.petName.id,
    });

    new TerraformOutput(this, "token", {
      value: naming.token.result,
      sensitive: true,
    });

    new TerraformOutput(this, "network_vpc_id", {
      value: network.getString("vpc_id"),
    });
  }
}

const app = new App();
new ModulesStack(app, "modules");
app.synth();
