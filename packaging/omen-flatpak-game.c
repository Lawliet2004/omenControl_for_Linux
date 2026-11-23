/* SPDX-License-Identifier: GPL-3.0-or-later
 * Run the original command in place, inside the existing Flatpak sandbox.
 * Build with static musl so launchers need no matching host libc or loader.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/vfs.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define GROUP_LIMIT 16384
#define HANDOFF_SECONDS 120
#define CGROUP2_MAGIC 0x63677270

static int fail(const char *message) {
    fprintf(stderr, "OMEN Flatpak wrapper: %s\n", message);
    return 1;
}

static int instance(char result[11]) {
    FILE *file = fopen("/.flatpak-info", "re");
    if (!file) return -1;
    char line[4096];
    int group = 0, found = 0, valid = 1;
    size_t total = 0;
    while (fgets(line, sizeof line, file)) {
        size_t len = strlen(line);
        total += len;
        if (total > 131072 || (len == sizeof line - 1 && line[len - 1] != '\n')) { valid = 0; break; }
        line[strcspn(line, "\r\n")] = 0;
        if (line[0] == '[') group = strcmp(line, "[Instance]") == 0;
        else if (group && strncmp(line, "instance-id=", 12) == 0) {
            const char *value = line + 12;
            size_t size = strlen(value);
            if (found || !size || size > 10 || strspn(value, "0123456789") != size || strtoull(value, NULL, 10) > UINT32_MAX || strtoull(value, NULL, 10) == 0) { valid = 0; break; }
            memcpy(result, value, size + 1);
            found = 1;
        }
    }
    if (ferror(file)) valid = 0;
    fclose(file);
    return valid && found ? 0 : -1;
}

static int membership(char result[GROUP_LIMIT + 1]) {
    FILE *file = fopen("/proc/self/cgroup", "re");
    if (!file) return -1;
    char line[GROUP_LIMIT + 1];
    int found = 0, valid = 1;
    while (fgets(line, sizeof line, file)) {
        size_t len = strlen(line);
        if (len == sizeof line - 1) { valid = 0; break; }
        if (strncmp(line, "0::", 3) == 0) {
            if (found || line[3] != '/') { valid = 0; break; }
            line[strcspn(line, "\r\n")] = 0;
            strcpy(result, line + 3);
            found = 1;
        }
    }
    if (ferror(file)) valid = 0;
    fclose(file);
    return valid && found ? 0 : -1;
}

static int64_t milliseconds(void) {
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value) != 0) return -1;
    return (int64_t)value.tv_sec * 1000 + value.tv_nsec / 1000000;
}

static int receive(int socket, char packet[34], int *descriptor, int64_t deadline) {
    *descriptor = -1;
    for (;;) {
        int64_t now = milliseconds();
        if (now < 0 || now >= deadline) return -1;
        struct pollfd fd = { .fd = socket, .events = POLLIN };
        int ready = poll(&fd, 1, (int)(deadline - now));
        if (ready < 0 && errno == EINTR) continue;
        if (ready <= 0 || !(fd.revents & POLLIN)) return -1;
        union { struct cmsghdr align; char bytes[CMSG_SPACE(8 * sizeof(int))]; } control;
        struct iovec iov = { .iov_base = packet, .iov_len = 34 };
        struct msghdr message = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = control.bytes, .msg_controllen = sizeof control.bytes };
        ssize_t size = recvmsg(socket, &message, MSG_CMSG_CLOEXEC);
        if (size < 0 && errno == EINTR) continue;
        int valid = size > 0 && !(message.msg_flags & (MSG_TRUNC | MSG_CTRUNC));
        for (struct cmsghdr *header = CMSG_FIRSTHDR(&message); header; header = CMSG_NXTHDR(&message, header)) {
            if (header->cmsg_level != SOL_SOCKET || header->cmsg_type != SCM_RIGHTS || header->cmsg_len < CMSG_LEN(0)) { valid = 0; continue; }
            size_t bytes = header->cmsg_len - CMSG_LEN(0);
            if (bytes % sizeof(int)) valid = 0;
            for (size_t i = 0; i < bytes / sizeof(int); ++i) {
                int fd;
                memcpy(&fd, CMSG_DATA(header) + i * sizeof(int), sizeof fd);
                if (*descriptor < 0) *descriptor = fd;
                else { close(fd); valid = 0; }
            }
        }
        if (!valid) { if (*descriptor >= 0) close(*descriptor); *descriptor = -1; return -1; }
        return (int)size;
    }
}

static int move_self(int fd, const char token[32], const char *original) {
    struct stat status;
    struct statfs fs;
    char link[64], target[GROUP_LIMIT + 1], after[GROUP_LIMIT + 1], expected[GROUP_LIMIT + 64];
    if (fstat(fd, &status) || !S_ISREG(status.st_mode) || fstatfs(fd, &fs) || fs.f_type != CGROUP2_MAGIC) return -1;
    snprintf(link, sizeof link, "/proc/self/fd/%d", fd);
    ssize_t size = readlink(link, target, sizeof target - 1);
    if (size < 0 || (size_t)size >= sizeof target - 1) return -1;
    target[size] = 0;
    const char *base = strrchr(target, '/');
    if (!base || strcmp(base, "/cgroup.procs") != 0) return -1;
    for (size_t i = 0; i < 32; ++i) if (!((token[i] >= '0' && token[i] <= '9') || (token[i] >= 'a' && token[i] <= 'f'))) return -1;
    // This FD grants only self-migration into one nested group. No numeric PID,
    // parent limits, namespace entry or access to the host cgroup mount is used.
    if (write(fd, "0\n", 2) != 2 || membership(after) != 0) return -1;
    snprintf(expected, sizeof expected, "%s%somen-game-%.32s.scope", original, strcmp(original, "/") ? "/" : "", token);
    return strcmp(after, expected) == 0 ? 0 : -1;
}

static int wait_helper(pid_t child, int64_t deadline) {
    for (;;) {
        int status;
        pid_t result = waitpid(child, &status, WNOHANG);
        if (result == child) return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : -1;
        // A parent that ignores SIGCHLD (or uses SA_NOCLDWAIT) auto-reaps its
        // helper. Approval already arrived over the private authenticated channel.
        if (result < 0 && errno == ECHILD) return 0;
        if (result < 0 && errno != EINTR) return -1;
        if (milliseconds() >= deadline) return -1;
        struct timespec pause = { .tv_nsec = 10000000 };
        nanosleep(&pause, NULL);
    }
}

int main(int argc, char **argv) {
    if (argc < 4 || strcmp(argv[2], "--") != 0) {
        fprintf(stderr, "Usage: omen-flatpak-game GAME_ID -- ACTUAL_COMMAND [ARGUMENTS...]\n");
        return 2;
    }
    char identity[11], original[GROUP_LIMIT + 1];
    if (instance(identity) || membership(original)) return fail("a running Flatpak instance is required");
    int sockets[2];
    if (socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, sockets)) return fail("cannot create private handoff channel");
    pid_t parent = getpid();
    int64_t now = milliseconds();
    if (now < 0) { close(sockets[0]); close(sockets[1]); return fail("monotonic clock unavailable"); }
    int64_t deadline = now + HANDOFF_SECONDS * 1000;
    pid_t child = fork();
    if (child < 0) { close(sockets[0]); close(sockets[1]); return fail("cannot start host helper"); }
    if (!child) {
        close(sockets[0]);
        // These changes affect only flatpak-spawn, never the original command.
        signal(SIGCHLD, SIG_DFL);
        signal(SIGTERM, SIG_DFL);
        sigset_t unblocked;
        sigemptyset(&unblocked); sigaddset(&unblocked, SIGTERM); sigaddset(&unblocked, SIGCHLD);
        sigprocmask(SIG_UNBLOCK, &unblocked, NULL);
        if (prctl(PR_SET_PDEATHSIG, SIGTERM) || getppid() != parent) _exit(1);
        if ((sockets[1] != 3 && dup2(sockets[1], 3) < 0) || fcntl(3, F_SETFD, 0)) _exit(1);
        if (sockets[1] != 3) close(sockets[1]);
        execl("/usr/bin/flatpak-spawn", "flatpak-spawn", "--host", "--watch-bus", "--directory=/", "--forward-fd=3",
            "/usr/bin/omen-cli", "game-flatpak-prepare", argv[1], "--instance", identity, "--handoff-fd", "3", (char *)NULL);
        _exit(127);
    }
    close(sockets[1]);
    int moved = 0, approved = 0;
    while (!approved) {
        char packet[34];
        int descriptor;
        int size = receive(sockets[0], packet, &descriptor, deadline);
        if (size == 33 && packet[0] == 'M' && descriptor >= 0 && !moved) {
            int result = move_self(descriptor, packet + 1, original);
            close(descriptor);
            if (result || send(sockets[0], "m", 1, MSG_NOSIGNAL) != 1) break;
            moved = 1;
        } else {
            if (descriptor >= 0) close(descriptor);
            if (size == 1 && descriptor < 0 && ((packet[0] == 'R' && moved) || (packet[0] == 'O' && !moved))) approved = 1;
            else break;
        }
    }
    close(sockets[0]);
    if (!approved || wait_helper(child, deadline)) return fail("preparation failed; the game was not started");
    execvp(argv[3], argv + 3);
    int error = errno;
    fprintf(stderr, "OMEN Flatpak wrapper: cannot execute command: %s\n", strerror(error));
    return error == ENOENT ? 127 : 126;
}
