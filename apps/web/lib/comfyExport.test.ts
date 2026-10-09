import { describe, expect, it } from "bun:test";
import type { ComfyExport, ComfyTarget } from "@/types/comfyui";
import {
	buildTargetPatch,
	chooseExportTarget,
	comfyErrorMessage,
	exportFailureMessage,
	exportPercent,
	isComfyExportableArtifact,
	isComfyExportableModel,
	isFileExistsFailure,
	normalizeBaseUrl,
	pollComfyExport,
	targetCheckState,
	validateTargetForm,
} from "./comfyExport";

const target = (id: string): ComfyTarget => ({
	id,
	name: id,
	baseUrl: "http://x:8188",
	hasToken: true,
	lastCheckAt: null,
	lastCheckOk: null,
	lastCheckError: null,
	createdAt: "",
	updatedAt: "",
});

const exp = (over: Partial<ComfyExport>): ComfyExport => ({
	id: "e1",
	targetId: "t1",
	targetName: "t1",
	filename: "a.safetensors",
	status: "queued",
	bytesTotal: 100,
	bytesSent: 0,
	error: null,
	errorCode: null,
	remotePath: null,
	createdAt: "",
	updatedAt: "",
	...over,
});

describe("chooseExportTarget", () => {
	it("sem destino pede cadastro", () => {
		expect(chooseExportTarget([]).kind).toBe("none");
	});
	it("um destino envia direto", () => {
		const c = chooseExportTarget([target("a")]);
		expect(c.kind === "direct" && c.target.id).toBe("a");
	});
	it("vários destinos abrem menu preservando a ordem", () => {
		const c = chooseExportTarget([target("a"), target("b")]);
		expect(c.kind === "menu" && c.targets.map((t) => t.id)).toEqual(["a", "b"]);
	});
});

describe("elegibilidade", () => {
	it("só LoRA de difusão no catálogo", () => {
		expect(isComfyExportableModel({ engine: "diffusion", kind: "lora" })).toBe(
			true,
		);
		expect(
			isComfyExportableModel({ engine: "diffusion", kind: "checkpoint" }),
		).toBe(false);
		expect(isComfyExportableModel({ engine: "yolo", kind: null })).toBe(false);
	});
	it("artefato .safetensors de treino de difusão", () => {
		const job = { kind: "diffusion", engine: "diffusion" } as const;
		expect(
			isComfyExportableArtifact(job, { path: "out/m_epoch_003.safetensors" }),
		).toBe(true);
		expect(isComfyExportableArtifact(job, { path: "out/m.pt" })).toBe(false);
		expect(
			isComfyExportableArtifact(
				{ kind: "yolo_train", engine: "yolo" },
				{ path: "w.safetensors" },
			),
		).toBe(false);
		expect(
			isComfyExportableArtifact(
				{ kind: "diffusion_generate", engine: "diffusion" },
				{ path: "w.safetensors" },
			),
		).toBe(false);
	});
});

describe("exportPercent", () => {
	it("proporção de bytes, limitada a 0..100", () => {
		expect(exportPercent(exp({ status: "uploading", bytesSent: 25 }))).toBe(25);
		expect(exportPercent(exp({ status: "uploading", bytesSent: 500 }))).toBe(
			100,
		);
		expect(exportPercent(exp({ status: "uploading", bytesTotal: 0 }))).toBe(0);
	});
	it("done é sempre 100", () => {
		expect(exportPercent(exp({ status: "done", bytesSent: 0 }))).toBe(100);
	});
});

describe("isFileExistsFailure", () => {
	it("só vale para export failed com errorCode file_exists", () => {
		expect(
			isFileExistsFailure({ status: "failed", errorCode: "file_exists" }),
		).toBe(true);
		expect(
			isFileExistsFailure({ status: "failed", errorCode: "network" }),
		).toBe(false);
		expect(isFileExistsFailure({ status: "failed", errorCode: null })).toBe(
			false,
		);
		expect(
			isFileExistsFailure({ status: "uploading", errorCode: "file_exists" }),
		).toBe(false);
	});
	it("não depende do texto da mensagem", () => {
		const e = exp({
			status: "failed",
			error: "o arquivo já existe",
			errorCode: "remote_error",
		});
		expect(isFileExistsFailure(e)).toBe(false);
	});
});

describe("comfyErrorMessage", () => {
	it("nome duplicado vem pelo code, não pelo status 409", () => {
		expect(comfyErrorMessage(409, "comfy_target_name_taken")).toContain("nome");
		expect(comfyErrorMessage(409, "outro")).not.toContain("nome");
	});
});

describe("pollComfyExport", () => {
	function run(seq: Array<ComfyExport | Error>, extra = {}) {
		const seen: string[] = [];
		const sleeps: number[] = [];
		let i = 0;
		const result = pollComfyExport("e1", {
			getExport: async () => {
				const next = seq[Math.min(i++, seq.length - 1)];
				if (next instanceof Error) throw next;
				return next;
			},
			sleep: async (ms) => {
				sleeps.push(ms);
			},
			onUpdate: (e) => seen.push(e.status),
			...extra,
		});
		return { result, seen, sleeps, calls: () => i };
	}

	it("segue queued → uploading → done a cada 1 s e para", async () => {
		const r = run([
			exp({ status: "queued" }),
			exp({ status: "uploading", bytesSent: 40 }),
			exp({ status: "done", bytesSent: 100 }),
		]);
		const out = await r.result;
		expect(out.kind).toBe("terminal");
		expect(r.seen).toEqual(["queued", "uploading", "done"]);
		expect(r.sleeps).toEqual([1000, 1000]);
		expect(r.calls()).toBe(3);
	});

	it("failed é terminal e devolve o erro do export", async () => {
		const out = await run([
			exp({
				status: "failed",
				error: "arquivo já existe",
				errorCode: "file_exists",
			}),
		]).result;
		expect(out.kind === "terminal" && out.export.errorCode).toBe("file_exists");
	});

	it("tolera erro transitório e retoma", async () => {
		const out = await run([new Error("net"), exp({ status: "done" })]).result;
		expect(out.kind).toBe("terminal");
	});

	it("desiste após erros consecutivos", async () => {
		const out = await run([new Error("net")], { maxConsecutiveErrors: 3 })
			.result;
		expect(out.kind).toBe("lost");
	});

	it("cancelamento interrompe sem notificar", async () => {
		let cancelled = false;
		const r = run([exp({ status: "uploading" })], {
			isCancelled: () => cancelled,
		});
		cancelled = true;
		expect((await r.result).kind).toBe("cancelled");
		expect(r.seen).toEqual([]);
	});
});

describe("formulário de destino", () => {
	it("normaliza barra final", () => {
		expect(normalizeBaseUrl(" http://h:8188/// ")).toBe("http://h:8188");
	});
	it("criação exige nome, URL http(s) sem userinfo e token", () => {
		const e = validateTargetForm(
			{ name: " ", baseUrl: "ftp://h", token: "" },
			false,
		);
		expect(Object.keys(e).sort()).toEqual(["baseUrl", "name", "token"]);
		expect(
			validateTargetForm(
				{ name: "a", baseUrl: "http://u:p@h", token: "t" },
				false,
			).baseUrl,
		).toBeDefined();
		expect(
			validateTargetForm(
				{ name: "a", baseUrl: "https://h/", token: "token-com-16-chars" },
				false,
			),
		).toEqual({});
	});
	it("edição aceita token vazio (manter)", () => {
		expect(
			validateTargetForm({ name: "a", baseUrl: "http://h", token: "" }, true),
		).toEqual({});
	});
	it("patch só leva o que mudou; token vazio é omitido", () => {
		const orig = { name: "a", baseUrl: "http://h" };
		expect(
			buildTargetPatch(orig, { name: "a", baseUrl: "http://h/", token: "" }),
		).toEqual({});
		expect(
			buildTargetPatch(orig, { name: "b", baseUrl: "http://h", token: " tk " }),
		).toEqual({
			name: "b",
			token: "tk",
		});
	});
});

describe("targetCheckState", () => {
	it("distingue nunca testado, ok e falha", () => {
		expect(targetCheckState({ lastCheckAt: null, lastCheckOk: null })).toBe(
			"never",
		);
		expect(targetCheckState({ lastCheckAt: "x", lastCheckOk: true })).toBe(
			"ok",
		);
		expect(targetCheckState({ lastCheckAt: "x", lastCheckOk: false })).toBe(
			"failed",
		);
	});
});

describe("exportFailureMessage", () => {
	it("traduz código conhecido ignorando o texto do servidor", () => {
		expect(
			exportFailureMessage({ error: "x", errorCode: "source_invalid" }),
		).toContain("inválido");
	});
	it("código desconhecido cai na mensagem do servidor", () => {
		expect(
			exportFailureMessage({ error: "algo novo", errorCode: "codigo_futuro" }),
		).toBe("algo novo");
		expect(exportFailureMessage({ error: null, errorCode: null })).toContain(
			"desconhecida",
		);
	});
});

describe("comfyErrorMessage do POST", () => {
	it("export_unsupported (422) explica o formato aceito", () => {
		expect(comfyErrorMessage(422, "export_unsupported")).toContain(
			"safetensors",
		);
	});
});

describe("token do destino", () => {
	it("rejeita token com menos de 16 caracteres", () => {
		const v = { name: "a", baseUrl: "http://h", token: "curto" };
		expect(validateTargetForm(v, false).token).toContain("16");
		expect(validateTargetForm(v, true).token).toContain("16");
		expect(validateTargetForm({ ...v, token: "x".repeat(16) }, false)).toEqual(
			{},
		);
	});
});

describe("novos códigos", () => {
	it("token_undecryptable manda reeditar o destino", () => {
		expect(
			exportFailureMessage({ error: null, errorCode: "token_undecryptable" }),
		).toContain("Edite o destino");
		expect(
			exportFailureMessage({ error: null, errorCode: "insufficient_storage" }),
		).toContain("espaço");
	});
});
