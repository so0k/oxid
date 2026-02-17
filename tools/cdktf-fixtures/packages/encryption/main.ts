// Adapted from TerraConstructs integ/aws/encryption: key.ts, key-alias.ts
import { App, LocalBackend, TerraformOutput } from "cdktf";
import { aws } from "terraconstructs";

const app = new App();
const stack = new aws.AwsStack(app, "encryption", {
  gridUUID: "12345678-1234",
  environmentName: "test",
  providerConfig: { region: "us-east-1" },
});
new LocalBackend(stack, { path: "encryption.tfstate" });

// --- Pattern 1: Key with resource policy and alias (from key.ts) ---
const masterKey = new aws.encryption.Key(stack, "MasterKey", {
  registerOutputs: true,
  outputName: "master_key",
});

masterKey.addToResourcePolicy(
  new aws.iam.PolicyStatement({
    resources: ["*"],
    actions: ["kms:encrypt"],
    principals: [new aws.iam.ArnPrincipal(stack.account)],
  }),
);

const masterAlias = masterKey.addAlias("alias/oxid-master");
new TerraformOutput(stack, "master_alias", {
  value: (masterAlias as any).aliasOutputs ?? masterAlias.aliasName,
  staticId: true,
});

// --- Pattern 2: Key created with alias in constructor (from key-alias.ts) ---
const aliasedKey = new aws.encryption.Key(stack, "AliasedKey", {
  alias: `OxidService${stack.account}`,
});
const aliasChild = aliasedKey.node.findChild("Alias") as aws.encryption.Alias;
new TerraformOutput(stack, "aliased_key_alias", {
  value: aliasChild.aliasOutputs,
  staticId: true,
});

// --- Pattern 3: Asymmetric key (from key.ts) ---
new aws.encryption.Key(stack, "AsymmetricKey", {
  keySpec: aws.encryption.KeySpec.ECC_NIST_P256,
  keyUsage: aws.encryption.KeyUsage.SIGN_VERIFY,
  registerOutputs: true,
  outputName: "asymmetric_key",
});

// --- Pattern 4: Key granted to a role ---
const encryptionRole = new aws.iam.Role(stack, "EncryptionRole", {
  assumedBy: new aws.iam.ServicePrincipal("lambda.amazonaws.com"),
  registerOutputs: true,
  outputName: "encryption_role",
});

masterKey.grantDecrypt(encryptionRole);
masterKey.grantEncrypt(encryptionRole);

app.synth();
