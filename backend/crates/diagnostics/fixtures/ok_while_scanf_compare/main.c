#include <stdio.h>

int main(void) {
    int x, sum = 0;
    while (scanf("%d", &x) == 1) {
        sum += x;
    }
    for (int i = 0; i < 3; i++) {
        sum += i;
    }
    printf("%d\n", sum);
    return 0;
}
