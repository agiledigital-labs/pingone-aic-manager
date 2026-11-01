export default class QueuedReporter {
  async onTestModuleQueued(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 300));
  }
}
