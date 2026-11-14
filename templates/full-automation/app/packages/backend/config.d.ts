export interface Config {
  awsBackstage: {
    /** The only Standard Step Functions workflow this portal can start. */
    rolloutStateMachineArn: string;
  };
}
