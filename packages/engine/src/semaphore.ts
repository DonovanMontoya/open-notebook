/** Runs at most `size` tasks at once, in the order they were queued. */
export class Semaphore {
  private active = 0
  private readonly waiting: Array<() => void> = []
  private readonly size: number

  constructor(size: number) {
    this.size = size
  }

  async run<T>(task: () => Promise<T>): Promise<T> {
    if (this.active >= this.size) await new Promise<void>((resolve) => this.waiting.push(resolve))
    this.active++
    try {
      return await task()
    } finally {
      this.active--
      this.waiting.shift()?.()
    }
  }
}
