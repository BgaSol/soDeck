import path from 'node:path';

export async function recordTrace(context, name) {
  if (!process.env.SODECK_CI_ARTIFACTS) return async () => {};
  await context.tracing.start({ screenshots: true, snapshots: true });
  let stopped = false;
  return async failed => {
    if (stopped) return;
    stopped = true;
    try {
      await context.tracing.stop(failed ? { path: path.join(process.env.SODECK_CI_ARTIFACTS, `${name}-trace.zip`) } : {});
    } catch (error) { console.error(`Could not save ${name} trace: ${error.message}`); }
  };
}
