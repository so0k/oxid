// Adapted from TerraConstructs integ/aws/compute: event-source-sqs.ts
import { App, LocalBackend, TerraformOutput } from "cdktf";
import { Construct } from "constructs";
import { aws } from "terraconstructs";

class ComputeEventsStack extends aws.AwsStack {
  constructor(scope: Construct, id: string, props: aws.AwsStackProps) {
    super(scope, id, props);

    // Lambda with SQS event source (from event-source-sqs.ts)
    const fn = new aws.compute.LambdaFunction(this, "Processor", {
      handler: "index.handler",
      code: aws.compute.Code.fromInline(
        `exports.handler = async (event) => { console.log(JSON.stringify(event)); return { statusCode: 200 }; }`,
      ),
      runtime: aws.compute.Runtime.NODEJS_18_X,
      loggingFormat: aws.compute.LoggingFormat.JSON,
      registerOutputs: true,
      outputName: "processor",
    });

    const dlq = new aws.notify.Queue(this, "DLQ", {
      registerOutputs: true,
      outputName: "dlq",
    });

    const queue = new aws.notify.Queue(this, "ProcessingQueue", {
      deadLetterQueue: {
        queue: dlq,
        maxReceiveCount: 3,
      },
      registerOutputs: true,
      outputName: "processing_queue",
    });

    const eventSource = new aws.compute.sources.SqsEventSource(queue, {
      batchSize: 5,
    });
    fn.addEventSource(eventSource);

    new TerraformOutput(this, "EventSourceMappingArn", {
      value: eventSource.eventSourceMappingArn,
      staticId: true,
    });

    // Second Lambda — API handler with queue grant
    const apiHandler = new aws.compute.LambdaFunction(this, "ApiHandler", {
      handler: "index.handler",
      code: aws.compute.Code.fromInline(
        `exports.handler = async (event) => ({ statusCode: 200, body: JSON.stringify({ message: "ok" }) });`,
      ),
      runtime: aws.compute.Runtime.NODEJS_18_X,
      registerOutputs: true,
      outputName: "api_handler",
    });

    queue.grantSendMessages(apiHandler);
  }
}

const app = new App();
new ComputeEventsStack(app, "compute-events", {
  gridUUID: "12345678-1234",
  environmentName: "test",
  providerConfig: { region: "us-east-1" },
});

app.synth();
