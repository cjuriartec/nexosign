import { describe, it, expect } from "vitest";
import type { BatchQueueStatus } from "$lib/stores/batch-queue.svelte";
import {
	batchStatusLabelCompact,
	batchStatusLabelFull,
	badgeVariantForBatchStatus,
	shortJobIdSidebar,
	shortJobIdWide,
} from "./queue-display";

describe("queue-display", () => {
	it("batchStatusLabelCompact y Full cubren todos los estados conocidos", () => {
		const statuses: BatchQueueStatus[] = [
			"preparing",
			"queued",
			"running",
			"cancelling",
			"cancelled",
			"finished",
			"error",
		];
		for (const s of statuses) {
			expect(batchStatusLabelCompact(s).length).toBeGreaterThan(0);
			expect(batchStatusLabelFull(s).length).toBeGreaterThan(0);
			const v = badgeVariantForBatchStatus(s);
			expect(["default", "secondary", "destructive", "outline"]).toContain(v);
		}
	});

	it("shortJobIdSidebar trunca ids largos", () => {
		expect(shortJobIdSidebar("short")).toBe("short");
		const long = "abcdefghijklmnopqr";
		expect(shortJobIdSidebar(long)).toBe(`${long.slice(0, 6)}…${long.slice(-4)}`);
		expect(shortJobIdSidebar(long).length).toBeLessThan(long.length);
	});

	it("shortJobIdWide usa límites distintos al sidebar", () => {
		const id = "x".repeat(30);
		const a = shortJobIdSidebar(id);
		const b = shortJobIdWide(id);
		expect(a).not.toBe(b);
		expect(shortJobIdWide("tiny")).toBe("tiny");
	});
});
