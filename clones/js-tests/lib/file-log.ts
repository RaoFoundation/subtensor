import fs from "node:fs";

const TEMP_DIR_URL = new URL("../temp/", import.meta.url);

export interface TempLogger {
  start(): Promise<void>;
  captureConsole(): void;
  info(...args: unknown[]): Promise<void>;
  error(value: unknown): Promise<void>;
  flush(): Promise<void>;
}

export function createTempLogger(filename: string): TempLogger {
  const fileUrl = new URL(filename, TEMP_DIR_URL);
  const buffer: string[] = [];
  let started = false;

  const append = (args: unknown[]): Promise<void> => {
    const line = `${args.map(formatValue).join(" ")}\n`;
    if (!started) {
      buffer.push(line);
      return Promise.resolve();
    }

    fs.appendFileSync(fileUrl, line);
    return Promise.resolve();
  };

  return {
    start() {
      if (started) {
        return Promise.resolve();
      }

      started = true;
      fs.mkdirSync(TEMP_DIR_URL, { recursive: true });
      fs.writeFileSync(fileUrl, buffer.join(""));
      buffer.length = 0;
      return Promise.resolve();
    },
    captureConsole() {
      console.log = (...args: unknown[]) => {
        void append(args);
      };
      console.error = (...args: unknown[]) => {
        void append(args);
      };
    },
    info(...args: unknown[]) {
      return append(args);
    },
    error(value: unknown) {
      return append([value]);
    },
    flush() {
      if (!started) {
        return this.start();
      }
      return Promise.resolve();
    },
  };
}

function formatValue(value: unknown): string {
  if (value instanceof Error) {
    return value.stack ?? value.message;
  }
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "bigint") {
    return value.toString();
  }
  return String(value);
}
