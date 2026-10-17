// Preserve gesture order across independent async backend workers. Cancelling
// discards pending work; an already dispatched native operation still finishes.
export class InputQueue {
  private tail: Promise<unknown> = Promise.resolve();
  private generation = 0;
  private outstanding = 0;

  get busy(): boolean { return this.outstanding > 0; }
  cancel(): void { this.generation++; }

  enqueue<T>(perform: () => Promise<T>): Promise<T | undefined> {
    const generation = this.generation;
    this.outstanding++;
    const result = this.tail.then(async () => {
      try {
        if (generation === this.generation) return await perform();
      } catch (error) {
        // A failed click must not be followed by typing into an unknown field.
        if (generation === this.generation) this.cancel();
        throw error;
      } finally { this.outstanding--; }
    });
    this.tail = result.catch(() => {});
    return result;
  }
}
