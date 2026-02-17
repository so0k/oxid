import { App, TerraformStack, TerraformIterator, TerraformVariable } from "cdktf";
import { Construct } from "constructs";
import { provider as randomProvider, pet, stringResource, integer } from "@cdktf/provider-random";
import { provider as nullProvider, resource as nullResource } from "@cdktf/provider-null";

/**
 * Demonstrates for_each patterns that generate tf.json fixtures:
 * - for_each over a list (via TerraformIterator.fromList)
 * - for_each over a map (via TerraformIterator.fromMap)
 * - chained for_each with cross-resource references
 */
class ForEachStack extends TerraformStack {
  constructor(scope: Construct, id: string) {
    super(scope, id);

    new randomProvider.RandomProvider(this, "random");
    new nullProvider.NullProvider(this, "null");

    // Variable: list of environment names
    const environments = new TerraformVariable(this, "environments", {
      type: "list(string)",
      default: ["dev", "staging", "prod"],
      description: "List of environments to create resources for",
    });

    // Variable: map of service configurations
    const services = new TerraformVariable(this, "services", {
      type: "map(object({ port = number, replicas = number }))",
      default: {
        api: { port: 8080, replicas: 3 },
        web: { port: 3000, replicas: 2 },
        worker: { port: 9090, replicas: 1 },
      },
      description: "Map of service names to their configurations",
    });

    // for_each over a list — random_pet per environment
    const envIterator = TerraformIterator.fromList(environments.listValue);

    new pet.Pet(this, "env_names", {
      forEach: envIterator,
      prefix: envIterator.value,
      length: 2,
    });

    // for_each over a map — random_string per service
    const svcIterator = TerraformIterator.fromMap(services.value);

    new stringResource.StringResource(this, "service_tokens", {
      forEach: svcIterator,
      length: 32,
      special: false,
    });

    // for_each over a static map — random_integer per region
    const regions: Record<string, string> = {
      us: "us-east-1",
      eu: "eu-west-1",
      ap: "ap-southeast-1",
    };
    const regionIterator = TerraformIterator.fromMap(regions);

    new integer.Integer(this, "region_seeds", {
      forEach: regionIterator,
      min: 1,
      max: 10000,
    });

    // Null resources with for_each demonstrating triggers
    const triggerIterator = TerraformIterator.fromList(["alpha", "beta", "gamma"]);

    new nullResource.Resource(this, "triggered", {
      forEach: triggerIterator,
      triggers: {
        name: triggerIterator.value,
        timestamp: "2026-01-01T00:00:00Z",
      },
    });
  }
}

const app = new App();
new ForEachStack(app, "foreach");
app.synth();
