#include <stdio.h>

int max(int a, int b) {
    if (a > b) {
        return a;
    }
}

int main(void) {
    printf("%d\n", max(1, 2));
    return 0;
}
