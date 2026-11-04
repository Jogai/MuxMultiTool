/** `items` with the one at `from` moved to `to`; the same list copied when either index is out of range. */
export function move<T>(items: readonly T[], from: number, to: number): T[] {
	const next = [...items];
	if (from === to || from < 0 || to < 0 || from >= next.length || to >= next.length) {
		return next;
	}
	const [item] = next.splice(from, 1);
	next.splice(to, 0, item as T);
	return next;
}

/** A copy of `set` with `item` added when it was absent and removed when it was present. */
export function toggled<T>(set: ReadonlySet<T>, item: T): Set<T> {
	const next = new Set(set);
	if (!next.delete(item)) {
		next.add(item);
	}
	return next;
}

const identity = (value: number): number => value;

/** The index of the first element of `sorted` (ascending by `key`) whose key is >= `value`; the length when none is. */
export function lowerBound<T>(sorted: readonly T[], value: number, key: (item: T) => number = identity as (item: T) => number): number {
	let low = 0;
	let high = sorted.length;
	while (low < high) {
		const mid = (low + high) >>> 1;
		if (key(sorted[mid]!) < value) {
			low = mid + 1;
		} else {
			high = mid;
		}
	}
	return low;
}

/** The index of the first element of `sorted` (ascending by `key`) whose key is > `value`; the length when none is. */
export function upperBound<T>(sorted: readonly T[], value: number, key: (item: T) => number = identity as (item: T) => number): number {
	let low = 0;
	let high = sorted.length;
	while (low < high) {
		const mid = (low + high) >>> 1;
		if (key(sorted[mid]!) <= value) {
			low = mid + 1;
		} else {
			high = mid;
		}
	}
	return low;
}
