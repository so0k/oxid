import { App, TerraformStack, TerraformOutput, TerraformVariable, TerraformLocal, Fn } from "cdktf";
import { Construct } from "constructs";
import { provider as randomProvider, pet, stringResource, password, uuid, shuffle, integer } from "@cdktf/provider-random";
import { provider as nullProvider, resource as nullResource } from "@cdktf/provider-null";
import { provider as localProvider, file, sensitiveFile } from "@cdktf/provider-local";
import { provider as tlsProvider, privateKey, selfSignedCert } from "@cdktf/provider-tls";
import { provider as timeProvider, sleep, staticResource } from "@cdktf/provider-time";

/**
 * Demonstrates multiple credential-free providers with varied resource types:
 * - random: pet, string, password, uuid, shuffle, integer
 * - null: resource with triggers and lifecycle
 * - local: file, sensitive_file
 * - tls: private_key, self_signed_cert
 * - time: sleep, static
 *
 * Also exercises: locals, variables, outputs, depends_on, lifecycle blocks
 */
class MultiProviderStack extends TerraformStack {
  constructor(scope: Construct, id: string) {
    super(scope, id);

    // --- Providers ---
    new randomProvider.RandomProvider(this, "random");
    new nullProvider.NullProvider(this, "null");
    new localProvider.LocalProvider(this, "local");
    new tlsProvider.TlsProvider(this, "tls");
    new timeProvider.TimeProvider(this, "time");

    // --- Variables ---
    const projectName = new TerraformVariable(this, "project_name", {
      type: "string",
      default: "oxid-multi-provider",
      description: "Name prefix for all resources",
    });

    const certValidityHours = new TerraformVariable(this, "cert_validity_hours", {
      type: "number",
      default: 8760,
      description: "Validity period for TLS certificates in hours",
    });

    const outputDir = new TerraformVariable(this, "output_dir", {
      type: "string",
      default: "${path.module}/generated",
      description: "Directory to write generated files to",
    });

    // --- Locals ---
    new TerraformLocal(this, "common_tags", {
      project: projectName.stringValue,
      managed_by: "cdktf",
      generated: true,
    });

    new TerraformLocal(this, "environments", ["dev", "staging", "prod"]);

    // --- Random provider resources ---
    const projectPet = new pet.Pet(this, "project_name_pet", {
      prefix: projectName.stringValue,
      length: 3,
      separator: "-",
    });

    const apiToken = new stringResource.StringResource(this, "api_token", {
      length: 48,
      special: true,
      overrideSpecial: "!@#$%",
      minLower: 8,
      minUpper: 8,
      minNumeric: 4,
      minSpecial: 2,
    });

    const dbPassword = new password.Password(this, "db_password", {
      length: 32,
      special: true,
      overrideSpecial: "!#$%&*()-_=+[]{}|:<>?",
    });

    const requestId = new uuid.Uuid(this, "request_id", {});

    const azShuffle = new shuffle.Shuffle(this, "az_order", {
      input: ["us-east-1a", "us-east-1b", "us-east-1c", "us-east-1d"],
      resultCount: 2,
    });

    const portNumber = new integer.Integer(this, "random_port", {
      min: 30000,
      max: 32767,
    });

    // --- TLS provider resources ---
    const caKey = new privateKey.PrivateKey(this, "ca_key", {
      algorithm: "RSA",
      rsaBits: 4096,
    });

    const serverKey = new privateKey.PrivateKey(this, "server_key", {
      algorithm: "ECDSA",
      ecdsaCurve: "P384",
    });

    const caCert = new selfSignedCert.SelfSignedCert(this, "ca_cert", {
      privateKeyPem: caKey.privateKeyPem,
      validityPeriodHours: certValidityHours.numberValue,
      isCaCertificate: true,
      allowedUses: ["cert_signing", "crl_signing"],
      subject: [
        {
          commonName: `${projectName.stringValue}-ca`,
          organization: "Oxid Test CA",
        },
      ],
    });

    // --- Time provider resources ---
    const creationTimestamp = new staticResource.StaticResource(this, "creation_time", {});

    const cooldown = new sleep.Sleep(this, "deploy_cooldown", {
      createDuration: "10s",
      destroyDuration: "5s",
      dependsOn: [caCert],
    });

    // --- Local file resources ---
    new file.File(this, "config_json", {
      filename: `${outputDir.stringValue}/config.json`,
      content: Fn.jsonencode({
        project: projectPet.id,
        port: portNumber.result,
        availability_zones: azShuffle.result,
        request_id: requestId.result,
        created_at: creationTimestamp.rfc3339,
      }),
      filePermission: "0644",
      directoryPermission: "0755",
      dependsOn: [cooldown],
    });

    new sensitiveFile.SensitiveFile(this, "secrets", {
      filename: `${outputDir.stringValue}/secrets.json`,
      content: Fn.jsonencode({
        api_token: apiToken.result,
        db_password: dbPassword.result,
        ca_private_key: caKey.privateKeyPem,
        server_private_key: serverKey.privateKeyPem,
      }),
      filePermission: "0600",
    });

    new file.File(this, "ca_cert_pem", {
      filename: `${outputDir.stringValue}/ca.pem`,
      content: caCert.certPem,
    });

    // --- Null resource with complex triggers ---
    new nullResource.Resource(this, "deployment_gate", {
      triggers: {
        config_hash: "${md5(local_file.config_json.content)}",
        ca_cert_serial: caCert.id,
        timestamp: creationTimestamp.rfc3339,
      },
      dependsOn: [cooldown],
    });

    // --- Outputs ---
    new TerraformOutput(this, "project_full_name", {
      value: projectPet.id,
      description: "Full project name with random pet suffix",
    });

    new TerraformOutput(this, "api_token_value", {
      value: apiToken.result,
      sensitive: true,
    });

    new TerraformOutput(this, "db_password_value", {
      value: dbPassword.result,
      sensitive: true,
    });

    new TerraformOutput(this, "selected_azs", {
      value: azShuffle.result,
      description: "Randomly selected availability zones",
    });

    new TerraformOutput(this, "ca_cert_output", {
      value: caCert.certPem,
      description: "CA certificate in PEM format",
    });

    new TerraformOutput(this, "server_public_key", {
      value: serverKey.publicKeyPem,
      description: "Server ECDSA public key",
    });

    new TerraformOutput(this, "random_port_output", {
      value: portNumber.result,
    });

    new TerraformOutput(this, "creation_timestamp", {
      value: creationTimestamp.rfc3339,
    });
  }
}

const app = new App();
new MultiProviderStack(app, "multi-provider");
app.synth();
