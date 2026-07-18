package hev.htproxy;

/** Narrow JNI boundary around the maintained hev-socks5-tunnel library. */
public final class TProxyService {
    private native void TProxyStartService(String configPath, int tunFd);
    private native void TProxyStopService();
    private native long[] TProxyGetStats();

    static {
        System.loadLibrary("hev-socks5-tunnel");
    }

    private boolean running;

    public synchronized void start(String configPath, int tunFd) {
        if (running) {
            throw new IllegalStateException("tun2socks is already running");
        }
        TProxyStartService(configPath, tunFd);
        running = true;
    }

    public synchronized void stop() {
        if (!running) {
            return;
        }
        TProxyStopService();
        running = false;
    }

    public synchronized long[] stats() {
        return running ? TProxyGetStats() : new long[] {0, 0, 0, 0};
    }
}
