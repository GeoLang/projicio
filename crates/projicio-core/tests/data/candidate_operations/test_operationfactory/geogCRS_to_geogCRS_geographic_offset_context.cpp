TEST(operation, geogCRS_to_geogCRS_geographic_offset_context) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("4120"), // NTF(Paris)
        authFactory->createCoordinateReferenceSystem("4121"), // NTF
        ctxt);
    ASSERT_EQ(list.size(), 1U);
    EXPECT_EQ(list[0]->nameStr(), "Greek to GGRS87 (1)");
    EXPECT_EQ(list[0]->exportToPROJString(PROJStringFormatter::create().get()),
              "+proj=pipeline +step +proj=axisswap +order=2,1 +step "
              "+proj=unitconvert +xy_in=deg +xy_out=rad +step +proj=geogoffset "
              "+dlat=-5.86 +dlon=0.28 +step +proj=unitconvert +xy_in=rad "
              "+xy_out=deg +step +proj=axisswap +order=2,1");
}

