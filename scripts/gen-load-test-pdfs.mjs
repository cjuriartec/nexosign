#!/usr/bin/env node
/**
 * Genera N PDFs válidos (~cabecera %PDF + xref) con un stream interno de relleno para acercarse
 * a un tamaño objetivo por archivo (prueba de carga Fase 5).
 *
 * Uso:
 *   node scripts/gen-load-test-pdfs.mjs [--out <dir>] [--count 100] [--bytes <n>] [--mb 10]
 *
 * Requiere Node 18+. Salida por defecto: scripts/load-test-pdfs/
 */

import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function usage() {
	console.error(`Uso: node scripts/gen-load-test-pdfs.mjs [opciones]

Opciones:
  --out <dir>     Directorio de salida (default: scripts/load-test-pdfs)
  --count <n>     Número de PDFs (default: 100)
  --mb <n>        Tamaño objetivo aproximado por fichero en MiB (default: 10)
  --bytes <n>     Tamaño objetivo exacto en bytes (tiene prioridad sobre --mb)
  -h, --help      Esta ayuda
`);
}

function parseArgs(argv) {
	let outDir = path.join(__dirname, "load-test-pdfs");
	let count = 100;
	let targetBytes = 10 * 1024 * 1024;
	let usedBytes = false;
	for (let i = 2; i < argv.length; i++) {
		const a = argv[i];
		if (a === "-h" || a === "--help") {
			usage();
			process.exit(0);
		}
		if (a === "--out") outDir = path.resolve(argv[++i]);
		else if (a === "--count") count = parseInt(argv[++i], 10);
		else if (a === "--mb") targetBytes = Math.round(parseFloat(argv[++i]) * 1024 * 1024);
		else if (a === "--bytes") {
			targetBytes = parseInt(argv[++i], 10);
			usedBytes = true;
		} else {
			console.error("Argumento desconocido:", a);
			usage();
			process.exit(1);
		}
	}
	if (!Number.isFinite(count) || count < 1 || count > 10_000) {
		console.error("--count debe estar entre 1 y 10000");
		process.exit(1);
	}
	if (!usedBytes && (!Number.isFinite(targetBytes) || targetBytes < 4096)) {
		console.error("--mb debe dar un tamaño >= unos pocos KiB");
		process.exit(1);
	}
	if (usedBytes && (!Number.isFinite(targetBytes) || targetBytes < 4096)) {
		console.error("--bytes debe ser >= 4096");
		process.exit(1);
	}
	return { outDir, count, targetBytes };
}

/** Ensambla un PDF 1.4 mínimo con un stream de contenido de longitud `streamPayloadLen`. */
function buildPdfBuffer(streamPayloadLen) {
	const head = Buffer.from("%PDF-1.4\n", "utf8");
	const obj1 = Buffer.from(
		"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
		"utf8",
	);
	const obj2 = Buffer.from(
		"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
		"utf8",
	);
	const obj3 = Buffer.from(
		"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>\nendobj\n",
		"utf8",
	);
	const obj4Open = Buffer.from(
		`4 0 obj\n<< /Length ${streamPayloadLen} >>\nstream\n`,
		"utf8",
	);
	const payload = Buffer.alloc(streamPayloadLen, 0x20);
	const obj4Close = Buffer.from("\nendstream\nendobj\n", "utf8");

	const body = Buffer.concat([head, obj1, obj2, obj3, obj4Open, payload, obj4Close]);

	const o1 = head.length;
	const o2 = o1 + obj1.length;
	const o3 = o2 + obj2.length;
	const o4 = o3 + obj3.length;

	const xrefLines = [
		"xref",
		"0 5",
		"0000000000 65535 f ",
		`${String(o1).padStart(10, "0")} 00000 n `,
		`${String(o2).padStart(10, "0")} 00000 n `,
		`${String(o3).padStart(10, "0")} 00000 n `,
		`${String(o4).padStart(10, "0")} 00000 n `,
	];
	const xrefStart = body.length;
	const trailer =
		xrefLines.join("\n") +
		"\ntrailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n" +
		xrefStart +
		"\n%%EOF\n";

	return Buffer.concat([body, Buffer.from(trailer, "utf8")]);
}

/** Ajusta streamPayloadLen para acercarse a targetTotalBytes. */
function pdfNearTargetBytes(targetTotalBytes) {
	let streamLen = Math.max(1, targetTotalBytes - 800);
	for (let iter = 0; iter < 12; iter++) {
		const buf = buildPdfBuffer(streamLen);
		const diff = targetTotalBytes - buf.length;
		if (Math.abs(diff) <= 64) return { buf, streamLen };
		streamLen = Math.max(1, streamLen + diff);
	}
	const buf = buildPdfBuffer(streamLen);
	return { buf, streamLen };
}

function main() {
	const { outDir, count, targetBytes } = parseArgs(process.argv);
	fs.mkdirSync(outDir, { recursive: true });

	const { buf: template } = pdfNearTargetBytes(targetBytes);
	const actual = template.length;
	console.error(
		`Plantilla: ~${(actual / (1024 * 1024)).toFixed(2)} MiB por fichero (${actual} bytes objetivo ${targetBytes})`,
	);

	for (let i = 0; i < count; i++) {
		const name = `load-${String(i + 1).padStart(4, "0")}.pdf`;
		const p = path.join(outDir, name);
		fs.writeFileSync(p, template);
	}

	console.error(`Escritos ${count} PDF en:\n  ${outDir}`);
	console.error(
		"\nSiguiente: export NEXOSIGN_BATCH_JOB_MAX_SECS=7200 (u otro valor ≥ tiempo total esperado), arranca NexoSign y firma la carpeta desde la UI o vía API.",
	);
}

main();
