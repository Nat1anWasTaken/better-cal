#include <stdio.h>

int main(void) {
    int sum;
    for (int i = 1; i <= 3; i++) {
        sum += i;
    }
    printf("%d\n", sum);
    return 0;
}
