#include <stdio.h>

int main(void) {
    int x, sum = 0;
    while (scanf("%d", &x)) {
        sum += x;
    }
    printf("%d\n", sum);
    return 0;
}
