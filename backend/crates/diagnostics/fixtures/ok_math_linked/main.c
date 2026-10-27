#include <stdio.h>
#include <math.h>

int main(void) {
    double x;
    if (scanf("%lf", &x) == 1) {
        printf("%.3f\n", sqrt(x));
    }
    return 0;
}
