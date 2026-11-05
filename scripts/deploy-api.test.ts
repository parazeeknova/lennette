import { afterEach, beforeEach, describe, expect, mock, spyOn, test } from "bun:test";
import { deploy } from "./deploy-api";

const image = `ghcr.io/parazeeknova/lennette-api@sha256:${"a".repeat(64)}`;
const previous = `ghcr.io/parazeeknova/lennette-api@sha256:${"b".repeat(64)}`;
const env = {
  DOKPLOY_URL: "https://dokploy.example",
  DOKPLOY_API_KEY: "private-test-key",
  DOKPLOY_APPLICATION_ID: "test-app",
  API_IMAGE: image,
  API_HEALTH_URL: "https://api.example/health/ready",
  DEPLOYMENT_TITLE: "test release",
};
const json = (value: unknown) => Response.json(value);
const noWait = async () => {};
beforeEach(() => {
  spyOn(console, "log").mockImplementation(() => {});
  spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => mock.restore());

function fixture(
  options: {
    previousImage?: string;
    deploymentError?: boolean;
    lostUpdateResponse?: boolean;
    inspectionFailure?: boolean;
    wrongImageOnce?: boolean;
    staleDeploymentOnce?: boolean;
    forwardHealth?: string | null;
    forwardHttp?: number;
    rollbackHttp?: number;
  } = {},
) {
  let desired = options.previousImage ?? previous;
  let running = desired;
  let title = "old release";
  let updated = false;
  let inspected = false;
  let listed = false;
  const deployments: string[] = [];
  const healthImages: string[] = [];
  const sleep = mock(noWait);
  const transport = mock(async (url: string, request?: RequestInit) => {
    const body = request?.body ? JSON.parse(String(request.body)) : {};
    if (url.includes("application.one")) return json({ dockerImage: desired, appName: "lennette" });
    if (url.includes("application.update")) {
      desired = body.dockerImage;
      if (options.lostUpdateResponse && !updated) {
        updated = true;
        throw new Error("Update response lost");
      }
      return json({});
    }
    if (url.includes("application.deploy")) {
      running = desired;
      title = body.title;
      deployments.push(running);
      return json({});
    }
    if (url.includes("deployment.all")) {
      if (options.staleDeploymentOnce && !listed) {
        listed = true;
        return json([{ title: "old release", status: "done" }]);
      }
      return json([
        { title, status: options.deploymentError && running === image ? "error" : "done" },
      ]);
    }
    if (url.includes("docker.getContainers"))
      return json([
        {
          name: "lennette.1.task",
          containerId: "test-container",
          state: "running",
          image: "ghcr.io/parazeeknova/lennette-api",
        },
      ]);
    if (url.includes("docker.getConfig")) {
      if (options.inspectionFailure && !inspected) {
        inspected = true;
        return new Response(null, { status: 404 });
      }
      if (options.wrongImageOnce && !inspected) {
        inspected = true;
        return json({ Config: { Image: previous }, State: { Health: { Status: "healthy" } } });
      }
      const status =
        running === image && "forwardHealth" in options ? options.forwardHealth : "healthy";
      return json({
        Config: { Image: running },
        State: { ...(status === null ? {} : { Health: { Status: status } }) },
      });
    }
    if (url === env.API_HEALTH_URL) {
      healthImages.push(running);
      return new Response(null, {
        status: running === image ? (options.forwardHttp ?? 204) : (options.rollbackHttp ?? 204),
      });
    }
    throw new Error("Unexpected request");
  });
  return { transport, sleep, deployments, healthImages, desired: () => desired };
}

describe("production deployment gates", () => {
  test("rejects mutable images and HTTP before sending credentials", async () => {
    const transport = mock(async () => json({}));
    await expect(
      deploy({ ...env, API_IMAGE: "ghcr.io/parazeeknova/lennette-api:latest" }, transport),
    ).rejects.toThrow("digest");
    await expect(
      deploy({ ...env, DOKPLOY_URL: "http://dokploy.example" }, transport),
    ).rejects.toThrow("HTTPS");
    expect(transport).not.toHaveBeenCalled();
  });

  test("failed deployment redeploys and verifies the previous running image", async () => {
    const f = fixture({ deploymentError: true });
    await expect(deploy(env, f.transport, f.sleep)).rejects.toThrow("deployment failed");
    expect(f.deployments).toEqual([image, previous]);
    expect(f.desired()).toBe(previous);
    expect(f.healthImages).toEqual([previous]);
    expect(console.error).toHaveBeenCalledWith(
      "Previous image redeployed; rollback digest and public readiness verified.",
    );
  });

  test("public readiness failure after a completed update rolls back the service", async () => {
    const f = fixture({ forwardHttp: 503 });
    await expect(deploy(env, f.transport, f.sleep)).rejects.toThrow("did not become ready");
    expect(f.deployments).toEqual([image, previous]);
    expect(f.healthImages.at(-1)).toBe(previous);
    expect(f.desired()).toBe(previous);
  });

  test("a committed image update with a lost response enters recovery", async () => {
    const f = fixture({ lostUpdateResponse: true });
    await expect(deploy(env, f.transport, f.sleep)).rejects.toThrow("Update response lost");
    expect(f.deployments).toEqual([previous]);
    expect(f.desired()).toBe(previous);
    expect(f.healthImages).toEqual([previous]);
  });

  test("a disappearing task is retried instead of rolling back a healthy release", async () => {
    const f = fixture({ inspectionFailure: true });
    await deploy(env, f.transport, f.sleep);
    expect(f.deployments).toEqual([image]);
    expect(f.sleep).toHaveBeenCalledTimes(1);
    expect(f.healthImages).toEqual([image]);
  });

  test("old deployment and healthy previous image cannot count as success", async () => {
    const f = fixture({ staleDeploymentOnce: true, wrongImageOnce: true });
    await deploy(env, f.transport, f.sleep);
    expect(f.sleep).toHaveBeenCalledTimes(2);
    expect(f.healthImages).toEqual([image]);
  });

  for (const health of ["starting", "unhealthy", null]) {
    test(`does not query public readiness for ${health ?? "absent"} container health`, async () => {
      const f = fixture({ previousImage: image, forwardHealth: health });
      await expect(deploy(env, f.transport, f.sleep)).rejects.toThrow("did not become ready");
      expect(f.healthImages).toEqual([]);
    });
  }

  test("failed rollback verification cannot report recovery", async () => {
    const f = fixture({ deploymentError: true, rollbackHttp: 503 });
    await expect(deploy(env, f.transport, f.sleep)).rejects.toThrow(
      "rollback could not be verified",
    );
    expect(console.error).not.toHaveBeenCalled();
  });

  test("errors never expose API response credentials", async () => {
    const transport = async () => new Response("private response credentials", { status: 403 });
    await expect(deploy(env, transport)).rejects.toThrow("HTTP 403");
    await expect(deploy(env, transport)).rejects.not.toThrow("private response credentials");
  });
});
