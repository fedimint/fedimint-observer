// Invalidate refresh responses as soon as cursor pagination starts.
export class PageRequests {
  private generation = 0;
  private paging = false;
  private paused = false;
  refresh(): number | null {
    return this.paging || this.paused ? null : this.generation;
  }
  acceptsRefresh(generation: number): boolean {
    return generation === this.generation && !this.paging && !this.paused;
  }
  beginPage(): number | null {
    if (this.paging) return null;
    this.paging = true;
    return ++this.generation;
  }
  finishPage(generation: number, success: boolean): boolean {
    if (generation !== this.generation || !this.paging) return false;
    this.paging = false;
    this.paused = success;
    return true;
  }
  reset(): void {
    this.generation++;
    this.paging = false;
    this.paused = false;
  }
}
